//! Port of `l/test/wait-render.test.ts` (pure helper subset).
//!
//! The reference renders job/IRC cards and checks soft-wrap and live snapshots
//! through a PTY harness. The crate ports the result-envelope stripping,
//! structured-preview flattening, row labels, and status tokens, which are
//! asserted here; card wrapping itself is host concern.

use maple_tui::tools::wait::{
	flatten_structured_preview, is_waiting_poll_details, job_row_label, render_call, render_result,
	status_color_token, status_icon_token, strip_task_result_envelope, AgentActivitySnapshot,
	CoordinationDetails, CoordinationOp, JobSnapshot, JobStatus, JobType,
};

fn job(id: &str) -> JobSnapshot {
	JobSnapshot {
		id: id.to_string(),
		..Default::default()
	}
}

#[test]
fn previews_the_task_result_envelope_body_not_the_wrapper_markup() {
	let delivery = "<task-result id=\"SpawnProbe\" status=\"completed\">\n\
		<output>Probe finished: spawned worker, ping ok.</output>\n\
		</task-result>\n\nSpawnProbe is now idle — message it via `write agent://SpawnProbe`";
	assert_eq!(
		strip_task_result_envelope(delivery),
		"Probe finished: spawned worker, ping ok."
	);
	assert!(!strip_task_result_envelope(delivery).contains("<task-result"));
	assert!(!strip_task_result_envelope(delivery).contains("<output>"));
}

#[test]
fn previews_a_truncated_preview_body_and_passes_non_envelopes_through() {
	let summary = "<task-result id=\"BigOne\">\n<preview>first line of long output</preview>\n</task-result>";
	assert_eq!(
		strip_task_result_envelope(summary),
		"first line of long output"
	);
	assert_eq!(strip_task_result_envelope("plain bash output"), "plain bash output");
	// Empty inner bodies fall back to the original text.
	assert_eq!(
		strip_task_result_envelope("<task-result><output>  </output></task-result>"),
		"<task-result><output>  </output></task-result>"
	);
}

#[test]
fn flattens_a_pretty_printed_json_body_onto_one_line() {
	assert_eq!(
		flatten_structured_preview("{\n  \"echo\": \"alpha\",\n  \"ok\": true\n}"),
		"{ \"echo\": \"alpha\", \"ok\": true }"
	);
	assert_eq!(flatten_structured_preview("[1,\n2]"), "[1, 2]");
	assert_eq!(flatten_structured_preview("not json"), "not json");
}

#[test]
fn labels_rows_with_stage_intent_or_id() {
	let mut running_task = job("SpawnProbe");
	running_task.r#type = Some(JobType::Task);
	running_task.status = Some(JobStatus::Running);
	running_task.stage = Some("patching".into());
	running_task.intent = Some("fix the parser".into());
	assert_eq!(job_row_label(&running_task), "patching");

	// A settled task falls back to intent, then label.
	running_task.status = Some(JobStatus::Completed);
	assert_eq!(job_row_label(&running_task), "fix the parser");
	running_task.intent = None;
	assert_eq!(job_row_label(&running_task), "SpawnProbe");

	let mut bash = job("Bash-1");
	bash.r#type = Some(JobType::Bash);
	bash.stage = Some("running command".into());
	assert_eq!(job_row_label(&bash), "Bash-1");
}

#[test]
fn maps_job_status_to_icon_and_color_tokens() {
	assert_eq!(status_icon_token(Some(JobStatus::Completed)), "success");
	assert_eq!(status_icon_token(Some(JobStatus::Failed)), "error");
	assert_eq!(status_icon_token(Some(JobStatus::Cancelled)), "aborted");
	assert_eq!(status_icon_token(Some(JobStatus::Running)), "running");
	assert_eq!(status_icon_token(None), "running");
	assert_eq!(status_color_token(Some(JobStatus::Completed)), "success");
	assert_eq!(status_color_token(Some(JobStatus::Failed)), "error");
	assert_eq!(status_color_token(Some(JobStatus::Cancelled)), "warning");
	assert_eq!(status_color_token(Some(JobStatus::Running)), "accent");
	assert_eq!(status_color_token(None), "accent");
}

#[test]
fn detects_running_only_poll_snapshots() {
	let running = CoordinationDetails {
		op: Some(CoordinationOp::Wait),
		jobs: vec![JobSnapshot { status: Some(JobStatus::Running), ..job("a") }],
		..Default::default()
	};
	assert!(is_waiting_poll_details(Some(&running)));
	assert!(!is_waiting_poll_details(None));

	let mixed = CoordinationDetails {
		jobs: vec![
			JobSnapshot { status: Some(JobStatus::Running), ..job("a") },
			JobSnapshot { status: Some(JobStatus::Completed), ..job("b") },
		],
		..Default::default()
	};
	assert!(!is_waiting_poll_details(Some(&mixed)));

	let cancelled = CoordinationDetails {
		jobs: vec![JobSnapshot { status: Some(JobStatus::Running), ..job("a") }],
		cancelled: vec![maple_tui::tools::wait::CancelOutcome {
			id: "a".into(),
			status: maple_tui::tools::wait::CancelStatus::Cancelled,
			message: String::new(),
		}],
		..Default::default()
	};
	assert!(!is_waiting_poll_details(Some(&cancelled)));
}

#[test]
fn renders_jobs_agents_and_overflow_counts() {
	assert_eq!(render_call(), "Wait");
	assert_eq!(render_result(None), vec!["No jobs".to_string()]);
	assert_eq!(
		render_result(Some(&CoordinationDetails::default())),
		vec!["No jobs".to_string()]
	);

	let details = CoordinationDetails {
		jobs: vec![
			JobSnapshot {
				id: "SpawnProbe".into(),
				status: Some(JobStatus::Completed),
				label: "SpawnProbe".into(),
				..Default::default()
			},
			JobSnapshot {
				id: "Other".into(),
				r#type: Some(JobType::Task),
				status: Some(JobStatus::Running),
				stage: Some("patching".into()),
				label: "Other".into(),
				..Default::default()
			},
		],
		agents: vec![AgentActivitySnapshot {
			id: "Anna".into(),
			activity: Some("editing src/lib.ts".into()),
			..Default::default()
		}],
		..Default::default()
	};
	let lines = render_result(Some(&details));
	assert!(lines.iter().any(|line| line.contains("SpawnProbe completed")));
	assert!(lines.iter().any(|line| line == "Other running - patching"));
	assert!(lines.iter().any(|line| line == "agent Anna editing src/lib.ts"));

	let many: Vec<JobSnapshot> = (0..25)
		.map(|index| JobSnapshot { id: format!("j{index}"), ..Default::default() })
		.collect();
	let lines = render_result(Some(&CoordinationDetails { jobs: many, ..Default::default() }));
	assert_eq!(lines.len(), 21);
	assert_eq!(lines.last().unwrap(), "… 5 more jobs");
}
