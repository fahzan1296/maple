//! GitHub tool render-model types and render/describe helpers.
//!
//! Reference: `l/src/tools/github.ts` (and shared formatting via
//! `crate::tools::gh_format` / `crate::tools::output_meta`).
//!
//! Deviations:
//! - TS renders live TUI components/native views with `Theme`, chalk, framed
//!   cards, status lines, ANSI log previews, `OwnerMemo`, and native node
//!   builders. Rust ports data types, op/title/meta derivation, watch/run/job/
//!   step state/progress logic, markdown section renderers, and plain
//!   describe/render-line helpers; live component/theme memoization is a host
//!   concern for the renderer-completion pass.
//! - `IsoBackendKind` is represented as `String` (no natives dependency).
//! - Sanitization uses tab/linebreak normalization and local helpers; full
//!   theme/symbol/glyph rendering uses plain text tokens.

use crate::tools::gh_format::{format_short_sha, push_line};
use crate::tools::output_meta::OutputMeta;

/// Tool args subset used for call rendering/meta.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GithubToolRenderArgs {
	pub op: Option<String>,
	pub run: Option<String>,
	pub branch: Option<String>,
	pub repo: Option<String>,
	pub pr: Option<GithubPrArg>,
	pub query: Option<String>,
}

/// `pr` arg may be string or list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GithubPrArg {
	One(String),
	Many(Vec<String>),
}

/// Display metadata for GitHub operations.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GhToolDetails {
	pub meta: Option<OutputMeta>,
	pub artifact_id: Option<String>,
	pub repo: Option<String>,
	pub branch: Option<String>,
	pub worktree_path: Option<String>,
	pub remote: Option<String>,
	pub remote_branch: Option<String>,
	pub head_sha: Option<String>,
	pub run_id: Option<u64>,
	pub run_ids: Option<Vec<u64>>,
	pub status: Option<String>,
	pub conclusion: Option<String>,
	pub failed_jobs: Option<Vec<String>>,
	pub watch: Option<GhRunWatchViewDetails>,
	pub checkouts: Option<Vec<GhPrCheckoutSummary>>,
}

/// Checkout location and branch metadata for a pull request.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GhPrCheckoutSummary {
	pub pr_number: Option<u64>,
	pub url: Option<String>,
	pub branch: String,
	pub worktree_path: String,
	pub remote: String,
	pub remote_branch: String,
	pub reused: bool,
	pub cloned_with: Option<String>,
}

/// Display state of one watched workflow step.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GhRunWatchStepDetails {
	pub number: u32,
	pub name: String,
	pub status: Option<String>,
	pub conclusion: Option<String>,
	pub duration_seconds: Option<u64>,
}

/// Display state of a watched workflow job.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GhRunWatchJobDetails {
	pub id: u64,
	pub name: String,
	pub status: Option<String>,
	pub conclusion: Option<String>,
	pub duration_seconds: Option<u64>,
	pub url: Option<String>,
	pub steps: Option<Vec<GhRunWatchStepDetails>>,
}

/// Display state of a watched workflow run.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GhRunWatchRunDetails {
	pub id: u64,
	pub workflow_name: Option<String>,
	pub display_title: Option<String>,
	pub status: Option<String>,
	pub conclusion: Option<String>,
	pub branch: Option<String>,
	pub head_sha: Option<String>,
	pub url: Option<String>,
	pub jobs: Vec<GhRunWatchJobDetails>,
}

/// Captured log preview for a failed job.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GhRunWatchFailedLogDetails {
	pub run_id: u64,
	pub workflow_name: Option<String>,
	pub job_name: String,
	pub conclusion: Option<String>,
	pub tail: Option<String>,
	pub available: bool,
	pub unavailable_reason: Option<String>,
}

/// Live workflow watcher display snapshot.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GhRunWatchViewDetails {
	pub mode: String,  // "run" | "commit"
	pub state: String, // "watching" | "completed"
	pub repo: String,
	pub branch: Option<String>,
	pub head_sha: Option<String>,
	pub poll_count: Option<u64>,
	pub note: Option<String>,
	pub run: Option<GhRunWatchRunDetails>,
	pub runs: Option<Vec<GhRunWatchRunDetails>>,
	pub failed_logs: Option<Vec<GhRunWatchFailedLogDetails>>,
}

/// Normalized status and timestamps of one workflow step.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GhRunStepSnapshot {
	pub number: u32,
	pub name: String,
	pub status: Option<String>,
	pub conclusion: Option<String>,
	pub started_at: Option<String>,
	pub completed_at: Option<String>,
}

/// Normalized status and timestamps of a workflow job.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GhRunJobSnapshot {
	pub id: u64,
	pub name: String,
	pub status: Option<String>,
	pub conclusion: Option<String>,
	pub started_at: Option<String>,
	pub completed_at: Option<String>,
	pub url: Option<String>,
	pub steps: Option<Vec<GhRunStepSnapshot>>,
}

/// Normalized workflow run and job snapshot.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GhRunSnapshot {
	pub id: u64,
	pub workflow_name: Option<String>,
	pub display_title: Option<String>,
	pub status: Option<String>,
	pub conclusion: Option<String>,
	pub branch: Option<String>,
	pub head_sha: Option<String>,
	pub created_at: Option<String>,
	pub updated_at: Option<String>,
	pub url: Option<String>,
	pub jobs: Vec<GhRunJobSnapshot>,
}

/// Captured failure logs associated with a run and job.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GhFailedJobLog {
	pub run: GhRunSnapshot,
	pub job: GhRunJobSnapshot,
	pub full: Option<String>,
	pub tail: Option<String>,
	pub available: bool,
	pub unavailable_reason: Option<String>,
	pub truncated: Option<bool>,
}

const SUCCESS_CONCLUSIONS: &[&str] = &["success", "neutral", "skipped"];
const FAILURE_CONCLUSIONS: &[&str] =
	&["failure", "timed_out", "cancelled", "action_required", "startup_failure"];
const RUNNING_STATUSES: &[&str] = &["in_progress"];
const PENDING_STATUSES: &[&str] = &["queued", "requested", "waiting", "pending"];

fn in_set(value: Option<&str>, set: &[&str]) -> bool {
	value.map(|v| set.contains(&v)).unwrap_or(false)
}

/// Replace tabs for display.
pub fn replace_tabs(text: &str) -> String {
	text.replace('\t', "   ")
}

/// Sanitize display line: replace tabs and collapse CR/LF.
pub fn sanitize_display_line(text: &str) -> String {
	replace_tabs(text).replace(['\r', '\n'], " ")
}

/// Truncate to width-ish chars with ellipsis.
pub fn truncate_visual(text: &str, max: usize) -> String {
	let count = text.chars().count();
	if count <= max {
		text.to_string()
	} else if max <= 1 {
		"…".to_string()
	} else {
		let mut out: String = text.chars().take(max - 1).collect();
		out.push('…');
		out
	}
}

/// Format operation title.
pub fn format_op_title(op: Option<&str>) -> String {
	match op {
		Some("repo_view") => "GitHub Repo".into(),
		Some("pr_checkout") => "GitHub PR Checkout".into(),
		Some("pr_push") => "GitHub PR Push".into(),
		Some("search_issues") => "GitHub Search Issues".into(),
		Some("search_prs") => "GitHub Search PRs".into(),
		Some("search_code") => "GitHub Search Code".into(),
		Some("search_commits") => "GitHub Search Commits".into(),
		Some("search_repos") => "GitHub Search Repos".into(),
		Some("run_watch") => "GitHub Run Watch".into(),
		_ => "GitHub".into(),
	}
}

/// Extract issue/PR id like `#123`.
pub fn extract_issue_id(value: Option<&str>) -> Option<String> {
	let trimmed = value?.trim();
	if trimmed.is_empty() {
		return None;
	}
	if trimmed.chars().all(|c| c.is_ascii_digit()) {
		return Some(format!("#{trimmed}"));
	}
	// /issues/123 or /pull/123
	for marker in ["/issues/", "/pull/"] {
		if let Some(idx) = trimmed.find(marker) {
			let rest = &trimmed[idx + marker.len()..];
			let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
			if !digits.is_empty() {
				return Some(format!("#{digits}"));
			}
		}
	}
	Some(truncate_visual(trimmed, 24))
}

/// Format PR identifier(s).
pub fn format_pr_identifier(pr: Option<&GithubPrArg>) -> Option<String> {
	match pr? {
		GithubPrArg::One(p) => extract_issue_id(Some(p)),
		GithubPrArg::Many(ps) => {
			let parts: Vec<String> = ps.iter().filter_map(|p| extract_issue_id(Some(p))).collect();
			if parts.is_empty() {
				return None;
			}
			if parts.len() > 3 {
				Some(format!(
					"{}, +{} more",
					parts[..3].join(", "),
					parts.len() - 3
				))
			} else {
				Some(parts.join(", "))
			}
		}
	}
}

/// Build operation meta labels.
pub fn build_op_meta(args: &GithubToolRenderArgs) -> Vec<String> {
	let mut meta = Vec::new();
	let op = args.op.as_deref();
	match op {
		Some("pr_checkout") | Some("pr_push") => {
			if let Some(id) = format_pr_identifier(args.pr.as_ref()) {
				meta.push(id);
			} else if let Some(branch) = &args.branch {
				meta.push(branch.clone());
			}
			if let Some(repo) = &args.repo {
				meta.push(repo.clone());
			}
		}
		Some("search_issues")
		| Some("search_prs")
		| Some("search_code")
		| Some("search_commits") => {
			if let Some(query) = &args.query {
				meta.push(truncate_visual(query, 60));
			}
			if let Some(repo) = &args.repo {
				meta.push(repo.clone());
			}
		}
		Some("search_repos") => {
			if let Some(query) = &args.query {
				meta.push(truncate_visual(query, 60));
			}
		}
		Some("repo_view") => {
			if let Some(repo) = &args.repo {
				meta.push(repo.clone());
			}
			if let Some(branch) = &args.branch {
				meta.push(branch.clone());
			}
		}
		Some("run_watch") => {}
		_ => {
			if let Some(repo) = &args.repo {
				meta.push(repo.clone());
			}
		}
	}
	meta
}

/// Watch header text.
pub fn get_watch_header(watch: &GhRunWatchViewDetails) -> String {
	if watch.mode == "run" {
		if let Some(run) = &watch.run {
			if watch.state == "watching" {
				return format!("watching run #{} on {}", run.id, watch.repo);
			}
			return format!("run #{} on {}", run.id, watch.repo);
		}
	}
	let short_sha = format_short_sha(watch.head_sha.as_deref()).unwrap_or("this commit");
	if watch.state == "watching" {
		format!("watching {short_sha} on {}", watch.repo)
	} else {
		format!("workflow runs for {short_sha} on {}", watch.repo)
	}
}

/// Run display label.
pub fn get_run_label(run: &GhRunWatchRunDetails) -> String {
	replace_tabs(
		run.workflow_name
			.as_deref()
			.or(run.display_title.as_deref())
			.unwrap_or("GitHub Actions"),
	)
}

/// Run meta parts.
pub fn get_run_meta(run: &GhRunWatchRunDetails) -> Vec<String> {
	let mut parts = Vec::new();
	if let Some(branch) = &run.branch {
		parts.push(replace_tabs(branch));
	} else if let Some(head) = &run.head_sha {
		parts.push(format_short_sha(Some(head)).unwrap_or(head.as_str()).to_string());
	}
	parts.push(format!("#{}", run.id));
	parts
}

/// First failed step in a list.
pub fn find_failed_step_watch(steps: Option<&[GhRunWatchStepDetails]>) -> Option<&GhRunWatchStepDetails> {
	steps?.iter().find(|step| {
		step.conclusion
			.as_deref()
			.map(|c| FAILURE_CONCLUSIONS.contains(&c))
			.unwrap_or(false)
	})
}

/// Step state label.
pub fn get_step_state_label(step: &GhRunWatchStepDetails) -> String {
	sanitize_display_line(
		step.conclusion
			.as_deref()
			.or(step.status.as_deref())
			.unwrap_or("unknown")
			.replace('_', " ")
			.as_str(),
	)
}

/// Job state tone token.
pub fn job_tone(job: &GhRunWatchJobDetails) -> &'static str {
	if in_set(job.conclusion.as_deref(), SUCCESS_CONCLUSIONS) {
		return "success";
	}
	if in_set(job.conclusion.as_deref(), FAILURE_CONCLUSIONS) {
		return "error";
	}
	if in_set(job.status.as_deref(), RUNNING_STATUSES) {
		return "warning";
	}
	"muted"
}

/// Step progress summary.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GhRunStepProgress {
	pub total: usize,
	pub completed: usize,
	pub current_step_name: Option<String>,
	pub current_step_number: Option<u32>,
	pub current_position: Option<usize>,
	pub failed_step_name: Option<String>,
	pub failed_step_number: Option<u32>,
	pub current_label: Option<String>,
	pub all_successful: bool,
}

/// Compute step progress for a watched job.
pub fn get_step_progress(job: &GhRunWatchJobDetails) -> Option<GhRunStepProgress> {
	let steps = job.steps.as_ref()?;
	if steps.is_empty() {
		return None;
	}
	let completed = steps
		.iter()
		.filter(|s| s.conclusion.is_some() || s.status.as_deref() == Some("completed"))
		.count();
	let current_index = steps
		.iter()
		.position(|s| s.conclusion.is_none() && s.status.as_deref() == Some("in_progress"));
	let failed = find_failed_step_watch(Some(steps));
	let next_index = if current_index.is_some() {
		current_index
	} else if failed.is_some() {
		None
	} else {
		steps.iter().position(|s| {
			s.conclusion.is_none()
				&& s.status
					.as_deref()
					.map(|st| st != "completed" && PENDING_STATUSES.contains(&st))
					.unwrap_or(false)
		})
	};
	let current = next_index.and_then(|i| steps.get(i));
	let all_successful = steps.iter().all(|s| {
		s.conclusion
			.as_deref()
			.map(|c| SUCCESS_CONCLUSIONS.contains(&c))
			.unwrap_or(false)
	});
	Some(GhRunStepProgress {
		total: steps.len(),
		completed,
		current_step_name: current.map(|s| sanitize_display_line(&s.name)),
		current_step_number: current.map(|s| s.number),
		current_position: current.and(next_index.map(|i| i + 1)),
		failed_step_name: failed.map(|s| sanitize_display_line(&s.name)),
		failed_step_number: failed.map(|s| s.number),
		current_label: current.map(get_step_state_label),
		all_successful,
	})
}

/// Step progress label.
pub fn get_step_progress_label(progress: &GhRunStepProgress) -> String {
	if let (Some(pos), Some(num), Some(name), Some(label)) = (
		progress.current_position,
		progress.current_step_number,
		&progress.current_step_name,
		&progress.current_label,
	) {
		return format!("{pos}/{} #{num} {name} · {label}", progress.total);
	}
	if let (Some(num), Some(name)) = (progress.failed_step_number, &progress.failed_step_name) {
		return format!("#{num} {name} · {}/{}", progress.completed, progress.total);
	}
	format!("{}/{} steps complete", progress.completed, progress.total)
}

/// Unavailable log message.
pub fn format_unavailable_log(entry: &GhRunWatchFailedLogDetails) -> String {
	let reason = entry
		.unavailable_reason
		.as_deref()
		.map(sanitize_display_line)
		.unwrap_or_default();
	if reason.is_empty() {
		"log tail unavailable".to_string()
	} else {
		format!("log tail unavailable: {reason}")
	}
}

/// Choose terminal conclusion or current job state.
pub fn format_job_state(job: &GhRunJobSnapshot) -> String {
	job.conclusion
		.as_deref()
		.or(job.status.as_deref())
		.unwrap_or("unknown")
		.to_string()
}

/// First failed step in snapshot.
pub fn find_failed_step_snapshot(steps: Option<&[GhRunStepSnapshot]>) -> Option<&GhRunStepSnapshot> {
	steps?.iter().find(|step| {
		step.conclusion
			.as_deref()
			.map(|c| FAILURE_CONCLUSIONS.contains(&c))
			.unwrap_or(false)
	})
}

/// Format workflow jobs as Markdown section lines.
pub fn render_jobs_section(jobs: &[GhRunJobSnapshot]) -> Vec<String> {
	if jobs.is_empty() {
		return vec![
			"## Jobs".to_string(),
			String::new(),
			"No jobs reported yet.".to_string(),
		];
	}
	let mut lines = vec![format!("## Jobs ({})", jobs.len()), String::new()];
	for job in jobs {
		lines.push(format!("- [{}] {}", format_job_state(job), job.name));
		if let Some(failed) = find_failed_step_snapshot(job.steps.as_deref()) {
			push_line(
				&mut lines,
				"  Failed step",
				Some(format!("#{} {}", failed.number, sanitize_display_line(&failed.name))),
			);
		}
		if let Some(v) = &job.started_at {
			push_line(&mut lines, "  Started", Some(v.as_str()));
		}
		if let Some(v) = &job.completed_at {
			push_line(&mut lines, "  Completed", Some(v.as_str()));
		}
		if let Some(v) = &job.url {
			push_line(&mut lines, "  URL", Some(v.as_str()));
		}
	}
	lines
}

/// Failed log render mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FailedLogMode {
	Tail { tail: usize },
	Full,
}

/// Format failed workflow logs as Markdown sections.
pub fn render_failed_job_logs(failed_job_logs: &[GhFailedJobLog], mode: FailedLogMode) -> Vec<String> {
	if failed_job_logs.is_empty() {
		return Vec::new();
	}
	let mut lines = vec!["## Failed Jobs".to_string(), String::new()];
	for entry in failed_job_logs {
		lines.push(format!(
			"### {} [{}]",
			entry.job.name,
			entry.job.conclusion.as_deref().unwrap_or("failed")
		));
		push_line(&mut lines, "Run", Some(format!("#{}", entry.run.id)));
		push_line(&mut lines, "Workflow", entry.run.workflow_name.as_deref());
		if let Some(v) = &entry.job.started_at {
			push_line(&mut lines, "Started", Some(v.as_str()));
		}
		if let Some(v) = &entry.job.completed_at {
			push_line(&mut lines, "Completed", Some(v.as_str()));
		}
		if let Some(v) = &entry.job.url {
			push_line(&mut lines, "URL", Some(v.as_str()));
		}
		lines.push(String::new());
		let log_text = match mode {
			FailedLogMode::Full => entry.full.as_deref(),
			FailedLogMode::Tail { .. } => entry.tail.as_deref(),
		};
		if entry.available {
			if let Some(text) = log_text {
				let header = match mode {
					FailedLogMode::Full => {
						if entry.truncated == Some(true) {
							"Partial log (capture truncated):"
						} else {
							"Full log:"
						}
					}
					FailedLogMode::Tail { tail } => {
						if entry.truncated == Some(true) {
							&format!("Last {tail} captured log lines (log capture truncated):")
						} else {
							&format!("Last {tail} log lines:")
						}
					}
				};
				lines.push(header.to_string());
				lines.push("```text".to_string());
				lines.push(text.to_string());
				lines.push("```".to_string());
			} else {
				lines.push(match mode {
					FailedLogMode::Full => "Full log unavailable.".to_string(),
					FailedLogMode::Tail { .. } => "Log tail unavailable.".to_string(),
				});
				if let Some(reason) = &entry.unavailable_reason {
					push_line(&mut lines, "Reason", Some(reason.as_str()));
				}
			}
		} else {
			lines.push(match mode {
				FailedLogMode::Full => "Full log unavailable.".to_string(),
				FailedLogMode::Tail { .. } => "Log tail unavailable.".to_string(),
			});
			if let Some(reason) = &entry.unavailable_reason {
				push_line(&mut lines, "Reason", Some(reason.as_str()));
			}
		}
		lines.push(String::new());
	}
	lines
}

/// Format a workflow run and its jobs as Markdown.
pub fn render_run_section(run: &GhRunSnapshot) -> Vec<String> {
	let label = if let Some(wf) = &run.workflow_name {
		format!("### Run #{} - {wf}", run.id)
	} else {
		format!("### Run #{}", run.id)
	};
	let mut lines = vec![label, String::new()];
	push_line(&mut lines, "Title", run.display_title.as_deref());
	push_line(&mut lines, "Branch", run.branch.as_deref());
	push_line(&mut lines, "Commit", format_short_sha(run.head_sha.as_deref()));
	push_line(&mut lines, "Status", run.status.as_deref());
	push_line(&mut lines, "Conclusion", run.conclusion.as_deref());
	push_line(&mut lines, "Created", run.created_at.as_deref());
	push_line(&mut lines, "Updated", run.updated_at.as_deref());
	push_line(&mut lines, "URL", run.url.as_deref());
	lines.push(String::new());
	lines.extend(render_jobs_section(&run.jobs));
	lines
}

/// Join tool result text blocks.
pub fn extract_text(content: &[serde_json::Value]) -> String {
	content
		.iter()
		.filter_map(|part| {
			if part.get("type").and_then(|v| v.as_str()) == Some("text") {
				part.get("text").and_then(|v| v.as_str()).map(|s| s.to_string())
			} else {
				None
			}
		})
		.collect::<Vec<_>>()
		.join("\n")
}

/// Describe call summary for GitHub tool.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GithubDescribeCall {
	pub title: String,
	pub meta: Vec<String>,
}

/// Build describe call.
pub fn describe_github_call(args: &GithubToolRenderArgs) -> GithubDescribeCall {
	let op = args.op.as_deref();
	if op == Some("run_watch") {
		let run_id = args.run.as_ref().filter(|s| !s.trim().is_empty()).cloned();
		let branch = args.branch.as_ref().filter(|s| !s.trim().is_empty()).cloned();
		let meta = if let Some(id) = run_id {
			format!("#{id}")
		} else {
			branch.unwrap_or_else(|| "current HEAD".to_string())
		};
		return GithubDescribeCall {
			title: "GitHub Run Watch".to_string(),
			meta: vec![meta],
		};
	}
	GithubDescribeCall {
		title: format_op_title(op),
		meta: build_op_meta(args),
	}
}

/// Plain describe result summary.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GithubDescribeResult {
	pub title: String,
	pub meta: Vec<String>,
	pub is_error: bool,
	pub is_warning: bool,
	pub watch_header: Option<String>,
	pub text: Option<String>,
	pub note: Option<String>,
}

/// Describe GitHub result (plain).
pub fn describe_github_result(
	args: &GithubToolRenderArgs,
	content: &[serde_json::Value],
	details: Option<&GhToolDetails>,
	is_error: bool,
) -> GithubDescribeResult {
	if let Some(watch) = details.and_then(|d| d.watch.as_ref()) {
		return GithubDescribeResult {
			title: "GitHub Run Watch".to_string(),
			meta: vec![get_watch_header(watch)],
			is_error,
			is_warning: false,
			watch_header: Some(get_watch_header(watch)),
			text: None,
			note: watch.note.clone(),
		};
	}
	let text = extract_text(content).trim().to_string();
	let title = format_op_title(args.op.as_deref());
	let meta = build_op_meta(args);
	if is_error {
		return GithubDescribeResult {
			title,
			meta,
			is_error: true,
			is_warning: false,
			watch_header: None,
			text: Some(if text.is_empty() { "request failed".into() } else { text }),
			note: None,
		};
	}
	if text.is_empty() {
		return GithubDescribeResult {
			title,
			meta,
			is_error: false,
			is_warning: true,
			watch_header: None,
			text: None,
			note: Some("no output".to_string()),
		};
	}
	GithubDescribeResult {
		title,
		meta,
		is_error: false,
		is_warning: false,
		watch_header: None,
		text: Some(text),
		note: None,
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn op_title_and_meta() {
		let args = GithubToolRenderArgs {
			op: Some("pr_checkout".into()),
			pr: Some(GithubPrArg::One("123".into())),
			repo: Some("o/r".into()),
			..Default::default()
		};
		assert_eq!(format_op_title(args.op.as_deref()), "GitHub PR Checkout");
		let meta = build_op_meta(&args);
		assert!(meta.iter().any(|m| m == "#123"));
		assert!(meta.iter().any(|m| m == "o/r"));
	}

	#[test]
	fn watch_header_and_run_meta() {
		let run = GhRunWatchRunDetails {
			id: 42,
			workflow_name: Some("CI".into()),
			branch: Some("main".into()),
			jobs: vec![],
			..Default::default()
		};
		let watch = GhRunWatchViewDetails {
			mode: "run".into(),
			state: "watching".into(),
			repo: "o/r".into(),
			run: Some(run.clone()),
			..Default::default()
		};
		assert_eq!(get_watch_header(&watch), "watching run #42 on o/r");
		assert_eq!(get_run_label(&run), "CI");
		assert_eq!(get_run_meta(&run), vec!["main".to_string(), "#42".to_string()]);
	}

	#[test]
	fn step_progress_current() {
		let job = GhRunWatchJobDetails {
			id: 1,
			name: "build".into(),
			steps: Some(vec![
				GhRunWatchStepDetails { number: 1, name: "a".into(), status: Some("completed".into()), conclusion: Some("success".into()), duration_seconds: None },
				GhRunWatchStepDetails { number: 2, name: "b".into(), status: Some("in_progress".into()), conclusion: None, duration_seconds: None },
			]),
			..Default::default()
		};
		let p = get_step_progress(&job).unwrap();
		assert_eq!(p.total, 2);
		assert_eq!(p.completed, 1);
		assert_eq!(p.current_position, Some(2));
		let label = get_step_progress_label(&p);
		assert!(label.contains("2/2"));
		assert!(label.contains("#2 b"));
	}

	#[test]
	fn render_run_section_markdown() {
		let run = GhRunSnapshot {
			id: 7,
			workflow_name: Some("CI".into()),
			status: Some("completed".into()),
			conclusion: Some("success".into()),
			jobs: vec![GhRunJobSnapshot {
				id: 1,
				name: "test".into(),
				status: Some("completed".into()),
				conclusion: Some("success".into()),
				..Default::default()
			}],
			..Default::default()
		};
		let lines = render_run_section(&run);
		let text = lines.join("\n");
		assert!(text.contains("### Run #7 - CI"));
		assert!(text.contains("## Jobs (1)"));
		assert!(text.contains("- [success] test"));
	}

	#[test]
	fn describe_result_watch() {
		let watch = GhRunWatchViewDetails {
			mode: "run".into(),
			state: "completed".into(),
			repo: "o/r".into(),
			run: Some(GhRunWatchRunDetails { id: 1, jobs: vec![], ..Default::default() }),
			..Default::default()
		};
		let details = GhToolDetails { watch: Some(watch), ..Default::default() };
		let r = describe_github_result(&GithubToolRenderArgs::default(), &[], Some(&details), false);
		assert_eq!(r.title, "GitHub Run Watch");
		assert!(r.watch_header.unwrap().contains("run #1"));
	}
}
