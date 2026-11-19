//! Port of `l/test/jobs-panel.test.ts`.
//!
//! The crate `JobsSheet` matches the reference roles for the split, detail,
//! list, command, output, summary, empty state and buttons. Two reference
//! details are adapted rather than asserted literally: the facts grid has no
//! `omp.jobs.facts` role (the `kv` builder carries only `items`), so facts are
//! found by `NodeKind::Kv`; and its label cells are muted spans, not plain
//! strings.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use maple_tui::keys::Key;
use maple_tui::native::node::{NativeNode, NodeKind};
use maple_tui::overlays::jobs_panel::{
	JobsPanelDetail, JobsPanelJob, JobsPanelSnapshot, JobsSheet, JobsSheetSource, JobStatus,
};
use serde_json::Value;

const NOW: i64 = 1_000_000;

fn job(id: &str, kind: &str, label: &str, status: JobStatus, agent: Option<&str>) -> JobsPanelJob {
	JobsPanelJob {
		id: id.to_string(),
		r#type: kind.to_string(),
		label: label.to_string(),
		intent: None,
		caption: None,
		status,
		start_time: NOW - 5_000,
		end_time: (!status.is_running()).then_some(NOW),
		owner_id: None,
		agent_id: agent.map(str::to_string),
	}
}

fn jobs() -> Vec<JobsPanelJob> {
	vec![
		job("task-1", "task", "Audit credits", JobStatus::Running, Some("CreditAudit")),
		job("bash-1", "bash", "cargo test --workspace", JobStatus::Running, None),
		job("bash-2", "bash", "bun lint", JobStatus::Failed, None),
	]
}

fn details() -> HashMap<String, JobsPanelDetail> {
	HashMap::from([
		(
			"task-1".to_string(),
			JobsPanelDetail {
				command: None,
				cwd: Some("/work".to_string()),
				pids: Vec::new(),
				exit_code: None,
				output: None,
				artifact_id: None,
				widget: None,
			},
		),
		(
			"bash-1".to_string(),
			JobsPanelDetail {
				command: Some("cargo test --workspace".to_string()),
				cwd: Some("/work".to_string()),
				pids: vec![41],
				exit_code: None,
				output: Some("running 12 tests\n".to_string()),
				artifact_id: None,
				widget: None,
			},
		),
		(
			"bash-2".to_string(),
			JobsPanelDetail {
				command: Some("bun lint".to_string()),
				cwd: Some("/work".to_string()),
				pids: Vec::new(),
				exit_code: Some(1),
				output: Some("1 error\n".to_string()),
				artifact_id: None,
				widget: None,
			},
		),
	])
}

#[derive(Clone)]
struct Source {
	snapshot: Rc<RefCell<JobsPanelSnapshot>>,
	details: Rc<RefCell<HashMap<String, JobsPanelDetail>>>,
	cancellable: bool,
	cancelled: Rc<RefCell<Vec<String>>>,
	closed: Rc<Cell<u32>>,
}

impl Source {
	fn new(snapshot: JobsPanelSnapshot, details: HashMap<String, JobsPanelDetail>) -> Self {
		Self {
			snapshot: Rc::new(RefCell::new(snapshot)),
			details: Rc::new(RefCell::new(details)),
			cancellable: true,
			cancelled: Rc::new(RefCell::new(Vec::new())),
			closed: Rc::new(Cell::new(0)),
		}
	}
}

impl JobsSheetSource for Source {
	fn load(&mut self) -> JobsPanelSnapshot {
		self.snapshot.borrow().clone()
	}

	fn inspect(&mut self, id: &str) -> Option<JobsPanelDetail> {
		self.details.borrow().get(id).cloned()
	}

	fn cancel(&mut self, id: &str) {
		self.cancelled.borrow_mut().push(id.to_string());
	}

	fn cancellable(&self) -> bool {
		self.cancellable
	}

	fn close(&mut self) {
		self.closed.set(self.closed.get() + 1);
	}
}

fn sheet_with(
	jobs: Vec<JobsPanelJob>,
	details: HashMap<String, JobsPanelDetail>,
) -> (JobsSheet, Source) {
	let snapshot = JobsPanelSnapshot {
		running: jobs,
		recent: Vec::new(),
		root_id: None,
		agents: Vec::new(),
	};
	let source = Source::new(snapshot, details);
	(JobsSheet::new(Box::new(source.clone())), source)
}

fn walk<'a>(node: &'a NativeNode, out: &mut Vec<&'a NativeNode>) {
	out.push(node);
	for child in &node.children {
		walk(child, out);
	}
}

fn by_role<'a>(root: &'a NativeNode, role: &str) -> Vec<&'a NativeNode> {
	let mut all = Vec::new();
	walk(root, &mut all);
	all.into_iter()
		.filter(|node| node.props.role() == Some(role))
		.collect()
}

fn texts(node: &NativeNode) -> Vec<String> {
	let mut all = Vec::new();
	walk(node, &mut all);
	all.into_iter()
		.filter_map(|n| n.props.get_str("text").map(str::to_string))
		.collect()
}

fn button_keys(root: &NativeNode) -> Vec<&str> {
	by_role(root, "omp.btn")
		.into_iter()
		.filter_map(|node| node.key.as_deref())
		.collect()
}

fn kv_items(root: &NativeNode) -> Vec<Value> {
	let mut all = Vec::new();
	walk(root, &mut all);
	all.into_iter()
		.find(|node| node.kind == NodeKind::Kv)
		.and_then(|node| node.props.get("items").and_then(Value::as_array).cloned())
		.unwrap_or_default()
}

fn kv_value_matches(item: &Value, label: &str, value: &str) -> bool {
	let key = item.get("k").and_then(Value::as_array);
	let val = item.get("v").and_then(Value::as_array);
	let key_ok = key.is_some_and(|spans| {
		spans
			.iter()
			.any(|span| span.get("t").and_then(Value::as_str) == Some(label))
	});
	let val_ok = val.is_some_and(|spans| {
		spans
			.iter()
			.any(|span| span.get("t").and_then(Value::as_str) == Some(value))
	});
	key_ok && val_ok
}

#[test]
fn lists_jobs_with_the_selected_detail_and_summary() {
	let (mut sheet, _source) = sheet_with(jobs(), details());
	let root = sheet.describe(NOW);

	let list = by_role(&root, "omp.jobs.list");
	assert_eq!(list.len(), 1);
	let ids: Vec<&str> = list[0]
		.children
		.iter()
		.filter_map(|child| child.key.as_deref())
		.collect();
	assert_eq!(ids, vec!["task-1", "bash-1", "bash-2"]);
	assert_eq!(list[0].props.get_str("selected"), Some("task-1"));

	let detail = by_role(&root, "omp.jobs.detail");
	assert_eq!(detail.len(), 1);
	assert!(texts(detail[0]).iter().any(|text| text == "Audit credits"));

	assert!(
		kv_items(&root)
			.iter()
			.any(|item| kv_value_matches(item, "Agent", "CreditAudit"))
	);
	assert!(
		by_role(&root, "omp.jobs.nooutput")
			.iter()
			.any(|node| node.props.get_str("text") == Some("No output yet"))
	);
	assert_eq!(
		by_role(&root, "omp.jobs.summary")[0].props.get_str("text"),
		Some("2 running · 1 recent")
	);
}

#[test]
fn arrow_keys_move_the_selection_and_swap_the_detail() {
	let (mut sheet, _source) = sheet_with(jobs(), details());
	sheet.handle_key(&Key::Down);
	let root = sheet.describe(NOW);
	assert_eq!(
		by_role(&root, "omp.jobs.list")[0].props.get_str("selected"),
		Some("bash-1")
	);
	assert_eq!(
		by_role(&root, "omp.jobs.command")[0].props.get_str("text"),
		Some("cargo test --workspace")
	);
	let output = by_role(&root, "omp.jobs.output");
	assert_eq!(output[0].props.get_str("text"), Some("running 12 tests\n"));
	assert_eq!(output[0].props.get_bool("follow"), Some(true));

	sheet.handle_key(&Key::Up);
	sheet.handle_key(&Key::Up);
	let root = sheet.describe(NOW);
	assert_eq!(
		by_role(&root, "omp.jobs.list")[0].props.get_str("selected"),
		Some("task-1")
	);
}

#[test]
fn cancel_buttons_offer_x_only_for_running_jobs() {
	let (mut sheet, source) = sheet_with(jobs(), details());
	sheet.handle_key(&Key::Down);
	assert_eq!(button_keys(&sheet.describe(NOW)), vec!["cancel", "close"]);
	sheet.handle_key(&Key::Char('x'));
	assert_eq!(source.cancelled.borrow().as_slice(), &["bash-1"][..]);

	sheet.handle_key(&Key::Down);
	assert_eq!(button_keys(&sheet.describe(NOW)), vec!["close"]);
	assert!(
		kv_items(&sheet.describe(NOW))
			.iter()
			.any(|item| kv_value_matches(item, "Exit code", "1"))
	);
	sheet.handle_key(&Key::Char('x'));
	assert_eq!(source.cancelled.borrow().len(), 1);
}

#[test]
fn empty_state_replaces_the_split_and_offers_only_close() {
	let (mut sheet, _source) = sheet_with(Vec::new(), HashMap::new());
	let root = sheet.describe(NOW);
	assert!(by_role(&root, "omp.jobs.split").is_empty());
	let empty = by_role(&root, "omp.jobs.empty");
	assert_eq!(empty.len(), 1);
	assert!(texts(empty[0]).iter().any(|text| text == "No background jobs"));
	assert_eq!(button_keys(&root), vec!["close"]);
	assert!(sheet
		.render(60, NOW)
		.iter()
		.any(|line| line.contains("No background jobs")));
}

#[test]
fn fallback_render_draws_the_tree_and_selected_detail() {
	let (mut sheet, _source) = sheet_with(jobs(), details());
	sheet.handle_key(&Key::Down);
	let lines = sheet.render(100, NOW);
	assert!(lines
		.iter()
		.any(|line| line.contains("Background jobs · 2 running")));
	assert!(lines.iter().any(|line| line.contains("pid 41")));
	assert!(lines.iter().any(|line| line.contains("running 12 tests")));
}
