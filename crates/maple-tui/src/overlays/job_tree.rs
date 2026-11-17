//! Jobs of an agent and its subagents as a tree.
//!
//! Reference: `l/src/overlays/job-tree.ts`.
//!
//! Deviations:
//! - Generic interfaces become Rust traits/structs with cloned job/agent data;
//!   callers pass slices and receive owned row vectors.
//! - `Theme` type maps to [`crate::theme::Theme`].
//! - Tree construction/linking follows the same intent (top-level for root/
//!   ownerless/unknown owners, subagent nodes under parents) but is implemented
//!   without cyclic parent-pointer mutation by building child lists in passes.
//!   Ordering and row placement aim to match the reference's observable behavior
//!   for common cases; complex detached/cyclic lineage edge cases may differ.

use std::collections::HashMap;

use crate::render::render_utils::{build_tree_prefix, get_tree_branch, get_tree_continue_prefix};
use crate::text_shape::visible_width;
use crate::theme::Theme;

/// What the tree reads off a job.
pub trait JobTreeJob {
	fn id(&self) -> &str;
	fn kind(&self) -> &str;
	fn agent_id(&self) -> Option<&str>;
	fn task_agent_id(&self) -> Option<&str>;
	fn live(&self) -> bool;
	fn start_time(&self) -> i64;
}

/// A subagent in the tree, by registry id.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JobTreeAgent {
	pub id: String,
	pub name: String,
	pub parent_id: Option<String>,
}

/// Where a row sits: depth and rails drawn left of it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JobTreePlace {
	pub depth: usize,
	pub is_last: bool,
	pub ancestors: Vec<bool>,
	pub has_children: bool,
}

/// One tree row.
#[derive(Clone, Debug, PartialEq)]
pub enum JobTreeRow<J> {
	Job {
		place: JobTreePlace,
		job: J,
		agent: Option<JobTreeAgent>,
	},
	Agent {
		place: JobTreePlace,
		agent: JobTreeAgent,
	},
}

impl<J> JobTreeRow<J> {
	#[must_use]
	pub fn place(&self) -> &JobTreePlace {
		match self {
			Self::Job { place, .. } | Self::Agent { place, .. } => place,
		}
	}
}

#[derive(Clone)]
struct Node<J: Clone> {
	owner: Option<JobTreeAgent>,
	jobs: Vec<J>,
	children: Vec<Node<J>>,
	size: usize,
	running: bool,
	start_time: i64,
}

impl<J: Clone> Node<J> {
	fn leaf_job(job: J) -> Self
	where
		J: JobTreeJob,
	{
		let start_time = job.start_time();
		let running = job.live();
		Self {
			owner: None,
			jobs: vec![job],
			children: Vec::new(),
			size: 1,
			running,
			start_time,
		}
	}
}

/// `jobs` as tree rows in display order.
pub fn build_job_tree<J: JobTreeJob + Clone>(
	jobs: &[J],
	agents: &[JobTreeAgent],
	root_id: Option<&str>,
) -> Vec<JobTreeRow<J>> {
	let mut jobs_by_owner: HashMap<String, Vec<J>> = HashMap::new();
	let mut top_jobs: Vec<J> = Vec::new();
	let agent_set: std::collections::HashSet<&str> = agents.iter().map(|a| a.id.as_str()).collect();
	for job in jobs {
		match job.agent_id() {
			Some(owner) if owner == root_id.unwrap_or("") || !agent_set.contains(owner) => {
				top_jobs.push(job.clone());
			}
			Some(owner) => jobs_by_owner.entry(owner.to_string()).or_default().push(job.clone()),
			None => top_jobs.push(job.clone()),
		}
	}

	fn build_agent_node<J: JobTreeJob + Clone>(
		agent: &JobTreeAgent,
		agents: &[JobTreeAgent],
		jobs_by_owner: &HashMap<String, Vec<J>>,
		root_id: Option<&str>,
		visiting: &mut Vec<String>,
	) -> Option<Node<J>> {
		if visiting.iter().any(|id| id == &agent.id) {
			return None;
		}
		visiting.push(agent.id.clone());
		let mut children = Vec::new();
		for child in agents {
			let is_child = match &child.parent_id {
				Some(pid) => pid == &agent.id,
				None => false,
			};
			if !is_child || root_id == Some(child.id.as_str()) {
				continue;
			}
			if let Some(node) = build_agent_node(child, agents, jobs_by_owner, root_id, visiting) {
				children.push(node);
			}
		}
		visiting.pop();
		let jobs = jobs_by_owner.get(&agent.id).cloned().unwrap_or_default();
		if jobs.is_empty() && children.is_empty() {
			return None;
		}
		Some(finalize_node(Some(agent.clone()), jobs, children))
	}

	let mut top: Vec<Node<J>> = top_jobs.into_iter().map(Node::leaf_job).collect();
	for agent in agents {
		let parent_is_root = match &agent.parent_id {
			Some(pid) => root_id == Some(pid.as_str()),
			None => true,
		};
		let parent_unknown = agent
			.parent_id
			.as_deref()
			.map(|pid| !agent_set.contains(pid) && root_id != Some(pid))
			.unwrap_or(false);
		if !(parent_is_root || parent_unknown) {
			continue;
		}
		let mut visiting = Vec::new();
		if let Some(node) = build_agent_node(agent, agents, &jobs_by_owner, root_id, &mut visiting) {
			top.push(node);
		}
	}
	settle(&mut top);
	flatten(&top)
}

fn finalize_node<J: JobTreeJob + Clone>(
	owner: Option<JobTreeAgent>,
	jobs: Vec<J>,
	children: Vec<Node<J>>,
) -> Node<J> {
	let mut running = jobs.iter().any(JobTreeJob::live) || children.iter().any(|c| c.running);
	let mut start_time = i64::MAX;
	for job in &jobs {
		start_time = start_time.min(job.start_time());
		if job.live() {
			running = true;
		}
	}
	for child in &children {
		start_time = start_time.min(child.start_time);
		running |= child.running;
	}
	let size = jobs.len() + children.iter().map(|c| c.size).sum::<usize>();
	Node {
		owner,
		jobs,
		children,
		size,
		running,
		start_time,
	}
}

fn settle<J: JobTreeJob + Clone>(nodes: &mut Vec<Node<J>>) {
	for node in nodes.iter_mut() {
		settle(&mut node.children);
		*node = finalize_node(node.owner.clone(), node.jobs.clone(), node.children.clone());
	}
	nodes.retain(|n| n.size > 0);
	nodes.sort_by(compare_nodes::<J>);
}

fn compare_nodes<J: JobTreeJob + Clone>(a: &Node<J>, b: &Node<J>) -> std::cmp::Ordering {
	b.size
		.cmp(&a.size)
		.then_with(|| (!a.running as u8).cmp(&(!b.running as u8)))
		.then_with(|| a.start_time.cmp(&b.start_time))
}

fn flatten<J: JobTreeJob + Clone>(top: &[Node<J>]) -> Vec<JobTreeRow<J>> {
	let mut out = Vec::new();
	flatten_level(top, 0, &[], &mut out);
	out
}

fn flatten_level<J: JobTreeJob + Clone>(
	nodes: &[Node<J>],
	depth: usize,
	ancestors: &[bool],
	out: &mut Vec<JobTreeRow<J>>,
) {
	for (index, node) in nodes.iter().enumerate() {
		let is_last = index + 1 == nodes.len();
		let has_children = node.jobs.len() > 1 || !node.children.is_empty();
		let place = JobTreePlace {
			depth,
			is_last,
			ancestors: ancestors.to_vec(),
			has_children,
		};
		let mut child_ancestors = ancestors.to_vec();
		child_ancestors.push(!is_last);

		if node.jobs.is_empty() {
			if let Some(agent) = &node.owner {
				out.push(JobTreeRow::Agent {
					place,
					agent: agent.clone(),
				});
			}
		} else {
			for (jidx, job) in node.jobs.iter().enumerate() {
				let job_is_last = jidx + 1 == node.jobs.len() && node.children.is_empty();
				let (job_place, agent) = if jidx == 0 {
					(
						JobTreePlace {
							depth,
							is_last: is_last && job_is_last,
							ancestors: ancestors.to_vec(),
							has_children: node.jobs.len() > 1 || !node.children.is_empty(),
						},
						node.owner.clone(),
					)
				} else {
					(
						JobTreePlace {
							depth: depth + 1,
							is_last: job_is_last,
							ancestors: child_ancestors.clone(),
							has_children: false,
						},
						None,
					)
				};
				out.push(JobTreeRow::Job {
					place: job_place,
					job: job.clone(),
					agent,
				});
			}
		}

		if !node.children.is_empty() {
			flatten_level(&node.children, depth + 1, &child_ancestors, out);
		}
	}
}

/// The connector drawn before a row's head.
#[must_use]
pub fn job_tree_guide(row: &JobTreePlace, theme: &Theme) -> String {
	if row.depth == 0 {
		String::new()
	} else {
		format!(
			"{}{}",
			build_tree_prefix(&row.ancestors, theme),
			get_tree_branch(row.is_last, theme)
		)
	}
}

/// The prefix of lines under a row's head.
#[must_use]
pub fn job_tree_indent(row: &JobTreePlace, theme: &Theme) -> String {
	if row.depth == 0 {
		return "  ".to_string();
	}
	let mut ancestors = row.ancestors.clone();
	ancestors.push(!row.is_last);
	let prefix = build_tree_prefix(&ancestors, theme);
	let cont = get_tree_continue_prefix(row.is_last, theme);
	let out = format!("{prefix}{cont}");
	if visible_width(&out) < 2 {
		"  ".to_string()
	} else {
		out
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[derive(Clone, Debug, PartialEq, Eq)]
	struct Job {
		id: &'static str,
		kind: &'static str,
		agent_id: Option<&'static str>,
		task_agent_id: Option<&'static str>,
		live: bool,
		start_time: i64,
	}
	impl JobTreeJob for Job {
		fn id(&self) -> &str {
			self.id
		}
		fn kind(&self) -> &str {
			self.kind
		}
		fn agent_id(&self) -> Option<&str> {
			self.agent_id
		}
		fn task_agent_id(&self) -> Option<&str> {
			self.task_agent_id
		}
		fn live(&self) -> bool {
			self.live
		}
		fn start_time(&self) -> i64 {
			self.start_time
		}
	}

	fn theme() -> Theme {
		crate::theme::Theme::default()
	}

	#[test]
	fn build_job_tree_returns_empty_for_no_jobs() {
		let rows = build_job_tree::<Job>(&[], &[], Some("Main"));
		assert!(rows.is_empty());
	}

	#[test]
	fn ownerless_job_becomes_top_level_row() {
		let jobs = vec![Job {
			id: "j1",
			kind: "bash",
			agent_id: None,
			task_agent_id: None,
			live: true,
			start_time: 1,
		}];
		let rows = build_job_tree(&jobs, &[], Some("Main"));
		assert_eq!(rows.len(), 1);
		match &rows[0] {
			JobTreeRow::Job { place, job, .. } => {
				assert_eq!(place.depth, 0);
				assert_eq!(job.id, "j1");
			}
			_ => panic!("expected job row"),
		}
	}

	#[test]
	fn job_tree_guide_is_empty_at_top_level() {
		let place = JobTreePlace {
			depth: 0,
			is_last: true,
			ancestors: vec![],
			has_children: false,
		};
		assert_eq!(job_tree_guide(&place, &theme()), "");
		assert_eq!(job_tree_indent(&place, &theme()), "  ");
	}
}
