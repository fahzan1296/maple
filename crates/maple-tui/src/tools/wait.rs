//! Wait/coordination tool render-model types and helpers.
//!
 //! Reference: `l/src/tools/wait.ts`.
 //!
 //! Deviations:
//! - JS-runtime-only facilities dropped: TUI `Component`/`Text` renderers,
//!   live job tiles/widgets, shimmered status, theme/spinner/width-aware
//!   layout, IRC/display cards, and interactive expansion. Module-doc only;
//!   parity provided via plain/native render+describe helpers.
 //! - TS renders live TUI components/native views with job tiles/widgets,
 //!   shimmered status, tree lists, and complex job/agent summaries. Rust ports
 //!   coordination/job/agent snapshot data models and pure helpers (status
//!   mapping, waiting-poll detection, task envelope stripping, preview
//!   flattening, label selection) plus exported render/describe helpers.
/// Coordination operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CoordinationOp {
	Send,
	Wait,
	Cancel,
}

impl CoordinationOp {
	pub fn as_str(self) -> &'static str {
		match self {
			Self::Send => "send",
			Self::Wait => "wait",
			Self::Cancel => "cancel",
		}
	}
}

/// Cancellation status.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CancelStatus {
	Cancelled,
	NotFound,
	AlreadyCompleted,
}

impl CancelStatus {
	pub fn as_str(self) -> &'static str {
		match self {
			Self::Cancelled => "cancelled",
			Self::NotFound => "not_found",
			Self::AlreadyCompleted => "already_completed",
		}
	}
}

/// Cancellation outcome.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CancelOutcome {
	pub id: String,
	pub status: CancelStatus,
	pub message: String,
}

/// Background job status.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JobStatus {
	Running,
	Completed,
	Failed,
	Cancelled,
}

impl JobStatus {
	pub fn as_str(self) -> &'static str {
		match self {
			Self::Running => "running",
			Self::Completed => "completed",
			Self::Failed => "failed",
			Self::Cancelled => "cancelled",
		}
	}
}

/// Background job type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JobType {
	Bash,
	Task,
	Eval,
}

impl JobType {
	pub fn as_str(self) -> &'static str {
		match self {
			Self::Bash => "bash",
			Self::Task => "task",
			Self::Eval => "eval",
		}
	}
}

/// Background-job snapshot (render-relevant subset).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct JobSnapshot {
	pub id: String,
	pub r#type: Option<JobType>,
	pub status: Option<JobStatus>,
	pub label: String,
	pub intent: Option<String>,
	pub duration_ms: u64,
	pub settled_at_ms: Option<i64>,
	pub exit_code: Option<i32>,
	pub resolved_model: Option<String>,
	pub stage: Option<String>,
	pub completion_percent: Option<u8>,
	pub result_text: Option<String>,
	pub error_text: Option<String>,
	pub agent_url_id: Option<String>,
	pub agent_output_published: Option<bool>,
}

/// Live subagent snapshot not represented by a job row.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AgentActivitySnapshot {
	pub id: String,
	pub parent_id: Option<String>,
	pub activity: Option<String>,
	pub age_ms: u64,
	pub live: bool,
	pub accepted_at: Option<i64>,
}

/// Coordination details for wait/cancel/send results.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CoordinationDetails {
	pub op: Option<CoordinationOp>,
	pub from: Option<String>,
	pub to: Option<String>,
	pub jobs: Vec<JobSnapshot>,
	pub cancelled: Vec<CancelOutcome>,
	pub agents: Vec<AgentActivitySnapshot>,
	pub interrupted: bool,
}

/// Whether a wait snapshot is a running-only poll with no cancellations.
pub fn is_waiting_poll_details(details: Option<&CoordinationDetails>) -> bool {
	let Some(d) = details else {
		return false;
	};
	if d.jobs.is_empty() {
		return false;
	}
	if !d.cancelled.is_empty() {
		return false;
	}
	d.jobs
		.iter()
		.all(|j| matches!(j.status, Some(JobStatus::Running)))
}

/// Strip task-result envelope, returning inner output/preview body when present.
pub fn strip_task_result_envelope(text: &str) -> String {
	if !text.starts_with("<task-result") {
		return text.to_string();
	}
	for tag in ["output", "preview"] {
		let open = format!("<{tag}");
		if let Some(start) = text.find(&open) {
			if let Some(gt) = text[start..].find('>') {
				let body_start = start + gt + 1;
				let close = format!("</{tag}>");
				if let Some(end_rel) = text[body_start..].find(&close) {
					let body = &text[body_start..body_start + end_rel];
					let body = body.trim_matches('\n');
					let trimmed = body.trim();
					if !trimmed.is_empty() {
						return trimmed.to_string();
					}
				}
			}
		}
	}
	text.to_string()
}

/// Flatten structured-looking JSON preview onto one line (bounded prefix).
pub fn flatten_structured_preview(text: &str) -> String {
	let first = text.chars().next();
	if first != Some('{') && first != Some('[') {
		return text.to_string();
	}
	let slice: String = text.chars().take(640).collect();
	slice.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Row label: running task stage, else intent, else label.
pub fn job_row_label(job: &JobSnapshot) -> &str {
	if matches!(job.r#type, Some(JobType::Task)) && matches!(job.status, Some(JobStatus::Running)) {
		if let Some(stage) = job.stage.as_deref().filter(|s| !s.trim().is_empty()) {
			return stage;
		}
	}
	job.intent
		.as_deref()
		.filter(|s| !s.is_empty())
		.unwrap_or(job.label.as_str())
}

/// Map job status to UI icon/status token.
pub fn status_icon_token(status: Option<JobStatus>) -> &'static str {
	match status {
		Some(JobStatus::Completed) => "success",
		Some(JobStatus::Failed) => "error",
		Some(JobStatus::Cancelled) => "aborted",
		Some(JobStatus::Running) | None => "running",
	}
}

/// Map job status to UI color token.
pub fn status_color_token(status: Option<JobStatus>) -> &'static str {
	match status {
		Some(JobStatus::Completed) => "success",
		Some(JobStatus::Failed) => "error",
		Some(JobStatus::Cancelled) => "warning",
		Some(JobStatus::Running) | None => "accent",
	}
}

/// Plain render of a wait call.
pub fn render_call() -> String {
	"Wait".to_string()
}

/// Plain render of a wait/coordination result.
pub fn render_result(details: Option<&CoordinationDetails>) -> Vec<String> {
	let Some(d) = details else { return vec!["No jobs".into()] };
	let mut lines = Vec::new();
	if d.jobs.is_empty() {
		lines.push("No jobs".into());
	} else {
		for j in d.jobs.iter().take(20) {
			let status = j.status.map(|s| s.as_str()).unwrap_or("unknown");
			lines.push(format!("{} {} - {}", j.id, status, job_row_label(j)));
		}
		if d.jobs.len() > 20 {
			lines.push(format!("… {} more jobs", d.jobs.len() - 20));
		}
	}
	for a in d.agents.iter().take(10) {
		lines.push(format!(
			"agent {} {}",
			a.id,
			a.activity.as_deref().unwrap_or("")
		));
	}
	lines
}

/// Native describe of a wait call.
pub fn describe_call() -> crate::tools::native_view::NativeToolView {
	use crate::native::node::{NodeText, Tone};
	use crate::tools::native_view::{tool_head, NativeToolView};
	NativeToolView {
		head: Some(NodeText::Spans(tool_head("Wait", &[]))),
		tone: Some(Tone::Accent),
		..NativeToolView::default()
	}
}

/// Native describe of a wait/coordination result.
pub fn describe_result(details: Option<&CoordinationDetails>) -> crate::tools::native_view::NativeToolView {
	use crate::native::describe::text as ntext;
	use crate::native::node::{NodeText, Props, Span, Tone};
	use crate::tools::native_view::{note_text, tool_head, NativeToolView, ToolHeadDetail};
	let mut head_details = Vec::new();
	if let Some(op) = details.and_then(|d| d.op) {
		head_details.push(ToolHeadDetail::Span(Span::styled(op.as_str().to_string(), "muted")));
	}
	if let Some(d) = details {
		head_details.push(ToolHeadDetail::Span(Span::styled(format!("{} jobs", d.jobs.len()), "muted")));
	}
	let head = tool_head("Wait", &head_details);
	let Some(d) = details else {
		return NativeToolView { head: Some(NodeText::Spans(head)), body: Some(vec![note_text("No jobs", "dim", None)]), ..NativeToolView::default() };
	};
	if d.jobs.is_empty() {
		return NativeToolView { head: Some(NodeText::Spans(head)), body: Some(vec![note_text("No jobs", "dim", None)]), ..NativeToolView::default() };
	}
	let mut children = Vec::new();
	for j in d.jobs.iter().take(30) {
		let status = j.status.map(|s| s.as_str()).unwrap_or("unknown");
		let mut p = Props::new();
		p.set_str("wrap", "word");
		children.push(ntext(NodeText::Spans(vec![
			Span::styled(format!("{} ", j.id), "accent"),
			Span::styled(format!("[{status}] ", ), "muted"),
			Span { t: job_row_label(j).to_string(), s: None, fx: None, href: None },
		]), p));
	}
	if d.jobs.len() > 30 {
		children.push(note_text(&format!("{} more jobs", d.jobs.len() - 30), "dim", None));
	}
	return NativeToolView { head: Some(NodeText::Spans(head)), body: Some(children), tone: Some(if is_waiting_poll_details(details) { Tone::Warning } else { Tone::Success }), ..NativeToolView::default() };
}

#[cfg(test)]
mod render_describe_tests {
	use super::*;
	#[test]
	fn call_and_result_helpers() {
		assert_eq!(render_call(), "Wait");
		let c = describe_call();
		assert!(c.head.is_some());
		let lines = render_result(None);
		assert_eq!(lines[0], "No jobs");
		let r = describe_result(None);
		assert!(r.body.is_some());
	}
}
#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn waiting_poll_detection() {
		let mut d = CoordinationDetails::default();
		assert!(!is_waiting_poll_details(Some(&d)));
		d.jobs.push(JobSnapshot {
			status: Some(JobStatus::Running),
			..Default::default()
		});
		assert!(is_waiting_poll_details(Some(&d)));
		d.jobs[0].status = Some(JobStatus::Completed);
		assert!(!is_waiting_poll_details(Some(&d)));
	}

	#[test]
	fn strip_task_envelope() {
		let s = "<task-result status=\"ok\">\n<output>\nhello\n</output>\n</task-result>";
		assert_eq!(strip_task_result_envelope(s), "hello");
		assert_eq!(strip_task_result_envelope("plain"), "plain");
	}

	#[test]
	fn flatten_and_label() {
		assert_eq!(flatten_structured_preview("{\n  \"a\": 1\n}"), "{ \"a\": 1 }");
		let job = JobSnapshot {
			r#type: Some(JobType::Task),
			status: Some(JobStatus::Running),
			label: "cmd".into(),
			stage: Some("thinking".into()),
			..Default::default()
		};
		assert_eq!(job_row_label(&job), "thinking");
	}

	#[test]
	fn tokens() {
		assert_eq!(status_icon_token(Some(JobStatus::Cancelled)), "aborted");
		assert_eq!(status_color_token(Some(JobStatus::Failed)), "error");
		assert_eq!(JobType::Eval.as_str(), "eval");
	}
}
