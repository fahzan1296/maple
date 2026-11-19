//! Port of `l/test/tools-native-github.test.ts` (portable subset).
//!
//! TS asserts live run-watch rendering (theme/width/layout, ANSI stripping in
//! rendered rows, clickable/expandable step/job native nodes, polling identity,
//! Ctrl+O hints). Rust exposes GitHub watch/header/progress/markdown/sanitize/
//! describe helpers and typed snapshots; this ports faithful contracts on those
//! APIs: step progress labels and sanitization, watch headers/meta, markdown job
//! sections including failed-step naming, and describe-call/result summaries.
//!
//! Deviations:
//! - No TUI component rendering, theme, visible-width/viewport layout, or
//!   hyperlink/OSC control-byte filtering in rendered ANSI rows; sanitization is
//!   asserted via `sanitize_display_line`/`replace_tabs`/progress labels.
//! - No native describe node graph for run-watch jobs/steps/keys/collapsed
//!   state or expansion toggling; watch describe is asserted via
//!   `describe_github_result`/header helpers.
//! - `renderJobsSection` model-facing markdown includes failed step names and
//!   is ported directly; UI-only per-step rows are not representable here.

use maple_tui::tools::gh_format::format_short_sha;
use maple_tui::tools::github::{
	build_op_meta, describe_github_call, describe_github_result, format_op_title, get_run_label,
	get_run_meta, get_step_progress, get_step_progress_label, get_watch_header, render_jobs_section,
	render_run_section, sanitize_display_line, GhRunJobSnapshot, GhRunSnapshot, GhRunStepSnapshot,
	GhRunWatchJobDetails, GhRunWatchRunDetails, GhRunWatchStepDetails, GhRunWatchViewDetails,
	GhToolDetails, GithubPrArg, GithubToolRenderArgs,
};

fn watch(mode: &str) -> GhRunWatchViewDetails {
	let run = GhRunWatchRunDetails {
		id: 77,
		workflow_name: Some("CI".to_string()),
		status: Some("in_progress".to_string()),
		jobs: vec![
			GhRunWatchJobDetails {
				id: 321,
				name: "Build job".to_string(),
				status: Some("in_progress".to_string()),
				duration_seconds: Some(14),
				steps: Some(vec![
					GhRunWatchStepDetails {
						number: 3,
						name: "Prepare".to_string(),
						status: Some("completed".to_string()),
						conclusion: Some("success".to_string()),
						duration_seconds: Some(1),
					},
					GhRunWatchStepDetails {
						number: 9,
						name: format!(
							"Compile\twith \u{001b}]52;c;clipboardpayload\u{0007} controls {}",
							"界".repeat(30)
						),
						status: Some("in_progress".to_string()),
						conclusion: None,
						duration_seconds: Some(8),
					},
					GhRunWatchStepDetails {
						number: 17,
						name: "Upload".to_string(),
						status: Some("queued".to_string()),
						conclusion: None,
						duration_seconds: None,
					},
				]),
				..GhRunWatchJobDetails::default()
			},
			GhRunWatchJobDetails {
				id: 322,
				name: "Unit tests".to_string(),
				status: Some("completed".to_string()),
				conclusion: Some("failure".to_string()),
				steps: Some(vec![
					GhRunWatchStepDetails {
						number: 2,
						name: "Run tests".to_string(),
						status: Some("in_progress".to_string()),
						conclusion: Some("cancelled".to_string()),
						duration_seconds: Some(3),
					},
					GhRunWatchStepDetails {
						number: 5,
						name: "Upload artifacts".to_string(),
						status: Some("queued".to_string()),
						conclusion: None,
						duration_seconds: None,
					},
				]),
				..GhRunWatchJobDetails::default()
			},
		],
		..GhRunWatchRunDetails::default()
	};
	if mode == "run" {
		GhRunWatchViewDetails {
			mode: "run".to_string(),
			state: "watching".to_string(),
			repo: "owner/repo".to_string(),
			run: Some(run),
			..GhRunWatchViewDetails::default()
		}
	} else {
		GhRunWatchViewDetails {
			mode: "commit".to_string(),
			state: "watching".to_string(),
			repo: "owner/repo".to_string(),
			head_sha: Some("abcdef123456".to_string()),
			runs: Some(vec![run]),
			..GhRunWatchViewDetails::default()
		}
	}
}

#[test]
fn watch_progress_and_sanitization_preserve_ordered_current_failed_step() {
	let w = watch("run");
	let run = w.run.as_ref().expect("run");
	assert_eq!(get_watch_header(&w), "watching run #77 on owner/repo");
	assert_eq!(get_run_label(run), "CI");
	assert_eq!(get_run_meta(run), vec!["#77".to_string()]);

	let build = &run.jobs[0];
	let progress = get_step_progress(build).expect("progress");
	assert_eq!(progress.total, 3);
	assert_eq!(progress.completed, 1);
	assert_eq!(progress.current_position, Some(2));
	assert_eq!(progress.current_step_number, Some(9));
	let label = get_step_progress_label(&progress);
	assert!(label.contains("2/3"));
	assert!(label.contains("#9"));
	assert!(label.contains("in progress"));
	// sanitize_display_line collapses tabs/CR/LF for labels; OSC payload bytes are
	// control-content in the raw name but not removed by this helper (terminal
	// renderers strip escapes separately). Assert tab/linebreak normalization.
	let current = progress.current_step_name.as_deref().unwrap_or("");
	assert!(!current.contains('\t'));
	assert!(!current.contains('\n'));
	assert_eq!(sanitize_display_line("a\tb\r\nc"), "a   b  c");

	let tests = &run.jobs[1];
	let failed = get_step_progress(tests).expect("failed progress");
	assert_eq!(failed.failed_step_number, Some(2));
	let failed_label = get_step_progress_label(&failed);
	assert!(failed_label.contains("#2 Run tests"));
	assert!(failed_label.contains("1/3"));
}

#[test]
fn watch_headers_call_meta_and_describe_follow_mode_and_result() {
	let commit_watch = watch("commit");
	assert_eq!(
		get_watch_header(&commit_watch),
		"watching abcdef123456 on owner/repo"
	);
	let completed = GhRunWatchViewDetails {
		state: "completed".to_string(),
		..commit_watch.clone()
	};
	assert_eq!(
		get_watch_header(&completed),
		"workflow runs for abcdef123456 on owner/repo"
	);
	assert_eq!(format_short_sha(Some("abcdef1234567890")), Some("abcdef123456"));

	let call = describe_github_call(&GithubToolRenderArgs {
		op: Some("run_watch".to_string()),
		run: Some("77".to_string()),
		..GithubToolRenderArgs::default()
	});
	assert_eq!(call.title, "GitHub Run Watch");
	assert_eq!(call.meta, vec!["#77".to_string()]);

	assert_eq!(format_op_title(Some("pr_checkout")), "GitHub PR Checkout");
	assert!(build_op_meta(&GithubToolRenderArgs {
		op: Some("pr_checkout".to_string()),
		pr: Some(GithubPrArg::One("123".to_string())),
		repo: Some("o/r".to_string()),
		..GithubToolRenderArgs::default()
	})
	.contains(&"#123".to_string()));

	let details = GhToolDetails {
		watch: Some(watch("run")),
		..GhToolDetails::default()
	};
	let desc = describe_github_result(
		&GithubToolRenderArgs {
			op: Some("run_watch".into()),
			..Default::default()
		},
		&[],
		Some(&details),
		false,
	);
	assert_eq!(desc.title, "GitHub Run Watch");
	assert!(desc.watch_header.unwrap_or_default().contains("run #77"));

	let err = describe_github_result(
		&GithubToolRenderArgs {
			op: Some("repo_view".into()),
			repo: Some("o/r".into()),
			..Default::default()
		},
		&[],
		None,
		true,
	);
	assert!(err.is_error);
	assert_eq!(err.title, "GitHub Repo");
}

#[test]
fn render_jobs_section_and_run_markdown_name_failed_step_for_model() {
	let jobs = vec![
		GhRunJobSnapshot {
			id: 5,
			name: "Checks".to_string(),
			conclusion: Some("failure".to_string()),
			steps: Some(vec![
				GhRunStepSnapshot {
					number: 1,
					name: "Set up job".to_string(),
					status: Some("completed".into()),
					conclusion: Some("success".into()),
					started_at: None,
					completed_at: None,
				},
				GhRunStepSnapshot {
					number: 4,
					name: "Run checks\nwith controls".to_string(),
					status: Some("completed".into()),
					conclusion: Some("failure".into()),
					started_at: None,
					completed_at: None,
				},
				GhRunStepSnapshot {
					number: 5,
					name: "Upload".to_string(),
					status: Some("completed".into()),
					conclusion: Some("skipped".into()),
					started_at: None,
					completed_at: None,
				},
			]),
			..GhRunJobSnapshot::default()
		},
		GhRunJobSnapshot {
			id: 6,
			name: "Lint".to_string(),
			conclusion: Some("success".to_string()),
			steps: Some(vec![GhRunStepSnapshot {
				number: 1,
				name: "Set up job".to_string(),
				status: Some("completed".into()),
				conclusion: Some("success".into()),
				started_at: None,
				completed_at: None,
			}]),
			..GhRunJobSnapshot::default()
		},
	];
	assert_eq!(
		render_jobs_section(&jobs),
		vec![
			"## Jobs (2)".to_string(),
			String::new(),
			"- [failure] Checks".to_string(),
			"  Failed step: #4 Run checks with controls".to_string(),
			"- [success] Lint".to_string(),
		]
	);
	let empty = render_jobs_section(&[]);
	assert_eq!(empty[0], "## Jobs");
	assert!(empty.join("\n").contains("No jobs reported yet"));

	let run = GhRunSnapshot {
		id: 7,
		workflow_name: Some("CI".to_string()),
		status: Some("completed".to_string()),
		conclusion: Some("success".to_string()),
		jobs: vec![GhRunJobSnapshot {
			id: 1,
			name: "test".to_string(),
			conclusion: Some("success".into()),
			..Default::default()
		}],
		..GhRunSnapshot::default()
	};
	let md = render_run_section(&run).join("\n");
	assert!(md.contains("### Run #7 - CI"));
	assert!(md.contains("## Jobs (1)"));
	assert!(md.contains("- [success] test"));
}
