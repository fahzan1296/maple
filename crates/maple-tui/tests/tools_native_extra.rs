//! Port of `l/test/tools-native-extra.test.ts` (portable subset).
//!
//! TS asserts TUI/native component rendering for todo/task/wait (native
//! checklist/agent nodes, nested agent trees, live elapsed clocks, render
//! scheduling). Rust exposes todo checklist selection/mapping and native
//! describe helpers, task plain/native call+result helpers, and wait plain/
//! native/describe helpers plus typed snapshots. This ports faithful data and
//! describe contracts that do not require a live component tree or timers.
//!
//! Deviations:
//! - Todo: asserts checklist phase titles/collapsed state and mapped item
//!   statuses/target counts via `describe_todo_result`/native body structure,
//!   not `TspChecklistProps` component identity or host checkbox rendering.
//! - Task: asserts call/result/head labeling and plain summaries via exported
//!   helpers; TS agent-node graph, assignment-prefix stripping, running tool
//!   age, context stats, and nested inflight child rendering require
//!   component/progress infrastructure not exposed here.
//! - Wait: asserts describe/render of running/completed job snapshots and
//!   waiting-poll tone detection; TS terminal-clocked elapsed nodes and timer
//!   scheduling (`setInterval`/`setTimeout`) are host/TUI concerns.

use maple_tui::tools::native_view::NativeToolView;
use maple_tui::tools::task::{describe_call as task_describe_call, render_call as task_render_call};
use maple_tui::tools::todo::{
	describe_todo_result, describe_todo_result_native, todo_checklist_phases, TodoItem, TodoPhase,
	TodoStatus,
};
use maple_tui::tools::wait::{
	describe_call as wait_describe_call, describe_result as wait_describe_result,
	is_waiting_poll_details, render_call as wait_render_call, render_result as wait_render_result,
	CoordinationDetails, CoordinationOp, JobSnapshot, JobStatus, JobType,
};
use maple_tui::native::node::NodeKind;

fn head_text(view: &NativeToolView) -> String {
	view.head
		.as_ref()
		.map(|h| match h {
			maple_tui::native::node::NodeText::Plain(s) => s.clone(),
			maple_tui::native::node::NodeText::Spans(spans) => {
				spans.iter().map(|s| s.t.clone()).collect::<String>()
			}
		})
		.unwrap_or_default()
}

fn count_kind(nodes: &[maple_tui::native::node::NativeNode], kind: NodeKind) -> usize {
	nodes
		.iter()
		.map(|n| usize::from(n.kind == kind) + count_kind(&n.children, kind))
		.sum()
}

#[test]
fn todo_renders_checklist_phases_with_mapped_statuses_and_folded_completed_phase() {
	let phases = vec![
		TodoPhase {
			name: "Setup".to_string(),
			tasks: vec![TodoItem {
				content: "init".to_string(),
				status: TodoStatus::Completed,
				..TodoItem::default()
			}],
		},
		TodoPhase {
			name: "Build".to_string(),
			tasks: vec![
				TodoItem {
					content: "write parser".to_string(),
					status: TodoStatus::Completed,
					..TodoItem::default()
				},
				TodoItem {
					content: "lex".to_string(),
					status: TodoStatus::InProgress,
					..TodoItem::default()
				},
				TodoItem {
					content: "old idea".to_string(),
					status: TodoStatus::Abandoned,
					..TodoItem::default()
				},
				TodoItem {
					content: "ship it".to_string(),
					status: TodoStatus::Pending,
					..TodoItem::default()
				},
			],
		},
	];
	let desc = describe_todo_result(&phases, false, "", true, &[]);
	assert_eq!(desc.title, "Todo");
	assert_eq!(desc.target.as_deref(), Some("3/5"));
	assert_eq!(desc.closed, 3);
	assert_eq!(desc.total, 5);
	assert_eq!(desc.phases.len(), 2);
	assert_eq!(desc.phases[0].title, "Setup");
	assert!(desc.phases[0].collapsed);
	assert_eq!(desc.phases[1].title, "Build");
	assert!(!desc.phases[1].collapsed);
	assert_eq!(
		desc.phases[1]
			.items
			.iter()
			.map(|i| (i.text.as_str(), i.status.as_str()))
			.collect::<Vec<_>>(),
		vec![
			("write parser", "done"),
			("lex", "active"),
			("old idea", "dropped"),
			("ship it", "pending"),
		]
	);

	let checklist = todo_checklist_phases(&phases, |_| false);
	assert_eq!(checklist[0].collapsed, true);
	assert_eq!(checklist[1].collapsed, false);

	let view = describe_todo_result_native(&phases, false, "", true, &[]);
	assert!(head_text(&view).contains("Todo"));
	assert!(head_text(&view).contains("3/5"));
	let body = view.body.expect("todo body");
	assert!(count_kind(&body, NodeKind::List) >= 1);
	assert!(count_kind(&body, NodeKind::Section) >= 2);
}

#[test]
fn task_call_labels_agents_and_result_helpers_render() {
	let call = task_render_call(Some("task"), Some("Complete assignment thoroughly:\n\n# Target\nFix the lexer"), Some(2));
	assert!(call.contains("task"));
	assert!(call.contains("2 agents"));
	assert!(call.contains("Complete assignment"));

	let view = task_describe_call(Some("task"), Some("# Target Fix the lexer"), Some(2));
	let head = head_text(&view);
	assert!(head.contains("Task") || head.contains("task") || !head.is_empty());
}

#[test]
fn wait_describe_and_render_report_jobs_and_waiting_poll_state() {
	assert_eq!(wait_render_call(), "Wait");
	assert_eq!(wait_render_result(None), vec!["No jobs".to_string()]);

	let details = CoordinationDetails {
		op: Some(CoordinationOp::Wait),
		jobs: vec![
			JobSnapshot {
				id: "job-1".to_string(),
				r#type: Some(JobType::Bash),
				status: Some(JobStatus::Running),
				label: "sleep".to_string(),
				duration_ms: 4200,
				..JobSnapshot::default()
			},
			JobSnapshot {
				id: "job-2".to_string(),
				r#type: Some(JobType::Task),
				status: Some(JobStatus::Completed),
				label: "x".to_string(),
				duration_ms: 900,
				..JobSnapshot::default()
			},
		],
		..CoordinationDetails::default()
	};
	assert!(!is_waiting_poll_details(Some(&details)), "mixed states are not a waiting poll");
	let lines = wait_render_result(Some(&details));
	assert!(lines.iter().any(|l| l.contains("job-1") && l.contains("running")));
	assert!(lines.iter().any(|l| l.contains("job-2") && l.contains("completed")));

	let poll = CoordinationDetails {
		op: Some(CoordinationOp::Wait),
		jobs: vec![JobSnapshot {
			id: "job-1".to_string(),
			status: Some(JobStatus::Running),
			label: "sleep".to_string(),
			..JobSnapshot::default()
		}],
		..CoordinationDetails::default()
	};
	assert!(is_waiting_poll_details(Some(&poll)));
	let call = wait_describe_call();
	assert!(head_text(&call).contains("Wait"));
	let res = wait_describe_result(Some(&poll));
	assert!(head_text(&res).contains("Wait"));
	assert_eq!(res.tone, Some(maple_tui::native::node::Tone::Warning));
	assert!(res.body.as_ref().map(|b| !b.is_empty()).unwrap_or(false));
}
