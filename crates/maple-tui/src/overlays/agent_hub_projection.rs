//! Agent hub roster projection and metrics aggregation.
//!
//! Reference: `l/src/overlays/agent-hub-projection.ts`.
//!
//! Deviations:
//! - `AgentRecordLike`/session/observer shapes are abstracted via local traits
//!   and lightweight structs because `@oh-my-pi/pi-agent-core` runtime types are
//!   not ported. Hosts adapt real records/sessions into these views.
//! - Session fallback reads (`getSessionStats`, `agent.state.messages`,
//!   `sessionManager.getUsageStatistics`, `model.contextWindow`) are obtained via
//!   trait methods returning owned snapshots; mutable cost patching of cached
//!   metrics is expressed by recomputing snapshots instead of mutating shared
//!   interior fields.
//! - `WeakMap<object, ...>` caches are represented as `HashMap<usize, ...>`
//!   keyed by host-provided stable cache ids (pointer identity or equivalent)
//!   because Rust has no JS weak-key identity map for arbitrary trait objects.

use std::collections::{HashMap, HashSet};

use crate::overlays::agent_hub_types::{AgentStatus, MAIN_AGENT_ID};

/// Metrics summary used by projection/renderer.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AgentMetrics {
	pub tokens: u64,
	pub requests: u64,
	pub tools: u64,
	pub cost: f64,
	pub duration_ms: u64,
	pub duration_kind: &'static str, // "active" | "span" | "unknown"
	pub context_tokens: Option<u64>,
	pub context_window: Option<u64>,
}

/// A snapshot that carries cost for row projection.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AgentCostSnapshot {
	pub cost: Option<f64>,
}

/// Aggregate metrics for the roster header.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AggregateMetrics {
	pub tokens: u64,
	pub requests: u64,
	pub tools: u64,
	pub cost: f64,
	pub duration_ms: u64,
	pub duration_kind: &'static str,
	pub reported_agents: usize,
	pub active_duration_agents: usize,
}

/// Observer progress view consumed by projection.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ObservedProgress {
	pub tokens: Option<f64>,
	pub requests: Option<f64>,
	pub tool_count: Option<f64>,
	pub cost: Option<f64>,
	pub duration_ms: Option<f64>,
	pub context_tokens: Option<f64>,
	pub context_window: Option<f64>,
}

/// Observable session view (subset).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ObservableSessionView {
	pub progress: Option<ObservedProgress>,
}

/// Minimal history view for a record.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AgentRecordHistoryView {
	pub metrics: Option<AgentMetrics>,
	pub direct_cost: Option<f64>,
	pub resolved_model: Option<String>,
	pub resolved_model_is_fallback: Option<bool>,
}

/// Minimal record view for projection.
pub trait AgentRecordView {
	fn id(&self) -> &str;
	fn parent_id(&self) -> Option<&str>;
	fn status(&self) -> AgentStatus;
	fn created_at(&self) -> i64;
	fn last_activity(&self) -> i64;
	fn history(&self) -> Option<AgentRecordHistoryView>;
	fn retained_metrics(&self) -> Option<AgentMetrics> {
		None
	}
	fn has_live_session(&self) -> bool {
		false
	}
	/// Stable key for fallback metrics caches (e.g. session pointer identity).
	fn fallback_cache_key(&self) -> Option<usize> {
		None
	}
	/// Optional live session fallback metrics source.
	fn fallback_live_metrics(&self) -> Option<AgentMetrics> {
		None
	}
	fn fallback_live_cost(&self) -> Option<f64> {
		None
	}
	fn fallback_stamp(&self) -> Option<FallbackStamp> {
		None
	}
}

/// Stamp of a fallback read to detect changes.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct FallbackStamp {
	pub messages_len: usize,
	pub last_shape: String,
	pub model_key: String,
	pub context_window: Option<u64>,
}

#[derive(Clone, Debug, Default, PartialEq)]
struct FallbackCacheEntry {
	stamp: Option<FallbackStamp>,
	metrics: Option<AgentMetrics>,
	cost: Option<f64>,
}

/// Mutable cache state keyed by host-provided ids.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ProjectionCache {
	entries: HashMap<usize, FallbackCacheEntry>,
}

impl ProjectionCache {
	#[must_use]
	pub fn new() -> Self {
		Self::default()
	}
}

/// Tree projection result.
#[derive(Clone, Debug)]
pub struct AgentTreeProjection<T> {
	pub rows: Vec<T>,
	pub depth_by_id: HashMap<String, usize>,
	pub parent_by_id: HashMap<String, String>,
	pub last_sibling_by_id: HashMap<String, bool>,
}

/// Status ordering for frozen ranks / grouping.
pub fn status_order(status: AgentStatus) -> u8 {
	match status {
		AgentStatus::Running => 0,
		AgentStatus::Idle => 1,
		AgentStatus::Parked => 2,
		AgentStatus::Aborted => 3,
	}
}

fn finite_u64(value: Option<f64>) -> Option<u64> {
	value.and_then(|v| if v.is_finite() && v >= 0.0 { Some(v as u64) } else { None })
}

fn finite_f64(value: Option<f64>) -> Option<f64> {
	value.filter(|v| v.is_finite())
}

/// Exact observer usage for one roster entry.
#[must_use]
pub fn progress_metrics(observed: Option<&ObservableSessionView>) -> Option<AgentMetrics> {
	let progress = observed.and_then(|o| o.progress.as_ref())?;
	let tokens = finite_f64(progress.tokens)?;
	let requests = finite_f64(progress.requests)?;
	let tools = finite_f64(progress.tool_count)?;
	let cost = finite_f64(progress.cost)?;
	let duration_ms = finite_f64(progress.duration_ms)?;
	Some(AgentMetrics {
		tokens: tokens as u64,
		requests: requests as u64,
		tools: tools as u64,
		cost,
		duration_ms: duration_ms as u64,
		duration_kind: "active",
		context_tokens: finite_u64(progress.context_tokens),
		context_window: finite_u64(progress.context_window),
	})
}

/// Live session key used for fallback stats caching in TS (`observed` ignored for key selection).
#[must_use]
pub fn hub_fallback_stats_key<T: AgentRecordView + ?Sized>(ref_: &T) -> Option<usize> {
	ref_.fallback_cache_key()
}

fn refresh_fallback<T: AgentRecordView + ?Sized>(cache: &mut ProjectionCache, key: usize, ref_: &T, refresh: bool) {
	let stamp = ref_.fallback_stamp();
	let needs = match cache.entries.get(&key) {
		None => true,
		Some(e) if refresh => e.stamp != stamp || e.metrics.is_none(),
		Some(_) => false,
	};
	if needs {
		let metrics = ref_.fallback_live_metrics();
		let cost = ref_.fallback_live_cost().or_else(|| metrics.as_ref().map(|m| m.cost));
		cache.entries.insert(
			key,
			FallbackCacheEntry {
				stamp,
				metrics,
				cost,
			},
		);
	}
}

/// Shared row policy for hub/status cost projection.
#[must_use]
pub fn project_row_metrics(
	history: Option<&AgentRecordHistoryView>,
	observed: Option<&ObservableSessionView>,
	live: Option<&AgentMetrics>,
	retained: Option<&AgentMetrics>,
	has_live_session: bool,
) -> Option<AgentMetrics> {
	let live_cost = live.map(|m| m.cost);
	let retained_cost = retained.map(|m| m.cost);
	let cost = live_cost
		.or_else(|| history.and_then(|h| h.direct_cost))
		.or(retained_cost);
	let metrics = if observed.and_then(|o| o.progress.as_ref()).is_some() {
		progress_metrics(observed)
	} else if let Some(hm) = history.and_then(|h| h.metrics.clone()) {
		Some(hm)
	} else if !has_live_session {
		retained.cloned().or_else(|| live.cloned())
	} else {
		live.cloned()
	};
	match (metrics, cost) {
		(Some(mut m), Some(c)) => {
			m.cost = c;
			Some(m)
		}
		(Some(m), None) => Some(m),
		_ => None,
	}
}

/// Full hub row metrics using a projection cache for fallback session reads.
#[must_use]
pub fn hub_row_metrics<T: AgentRecordView + ?Sized>(
	ref_: &T,
	observed: Option<&ObservableSessionView>,
	cache: &mut ProjectionCache,
	refresh_fallback_reads: bool,
) -> Option<AgentMetrics> {
	let history = ref_.history();
	let retained = ref_.retained_metrics();
	let mut live: Option<AgentMetrics> = None;
	if let Some(key) = hub_fallback_stats_key(ref_) {
		refresh_fallback(cache, key, ref_, refresh_fallback_reads);
		live = cache.entries.get(&key).and_then(|e| e.metrics.clone());
		if live.is_some() {
			cache.entries.insert(
				key.wrapping_add(usize::MAX / 2), // not used; keep retained separately below
				FallbackCacheEntry::default(),
			);
			// retain a copy under record-local key? use separate map conceptually; simplest: store by key and also allow caller to key retained
		}
	}
	project_row_metrics(
		history.as_ref(),
		observed,
		live.as_ref(),
		retained.as_ref(),
		ref_.has_live_session(),
	)
}

/// Cost-only aggregation with fallback session cache.
#[must_use]
pub fn aggregate_cost<T, F>(rows: &[T], observed_for: F, cache: &mut ProjectionCache) -> f64
where
	T: AgentRecordView,
	F: Fn(&str) -> Option<ObservableSessionView>,
{
	let mut total = 0.0;
	let mut counted_sessions: HashSet<usize> = HashSet::new();
	for ref_ in rows {
		let observed = observed_for(ref_.id());
		let session_key = hub_fallback_stats_key(ref_);
		if let Some(key) = session_key {
			refresh_fallback(cache, key, ref_, true);
		}
		let live = session_key.and_then(|k| cache.entries.get(&k)).and_then(|e| e.metrics.clone());
		let history = ref_.history();
		let cost = project_row_metrics(
			history.as_ref(),
			observed.as_ref(),
			live.as_ref(),
			ref_.retained_metrics().as_ref(),
			ref_.has_live_session(),
		)
		.map(|m| m.cost);
		if let Some(key) = session_key {
			if let Some(c) = cost {
				if counted_sessions.contains(&key) {
					continue;
				}
				counted_sessions.insert(key);
				total += c;
			}
		} else if let Some(c) = cost {
			total += c;
		}
	}
	total
}

/// Aggregate metrics across rows.
#[must_use]
pub fn aggregate_metrics<T, F>(
	rows: &[T],
	observed_for: F,
	cache: &mut ProjectionCache,
	refresh_fallback_reads: bool,
) -> (AggregateMetrics, bool)
where
	T: AgentRecordView,
	F: Fn(&str) -> Option<ObservableSessionView>,
{
	let mut total = AggregateMetrics {
		duration_kind: "active",
		..Default::default()
	};
	let mut has_fallback_live_sessions = false;
	let mut counted_fallback_sessions: HashSet<usize> = HashSet::new();
	for ref_ in rows {
		let observed = observed_for(ref_.id());
		let fallback_key = hub_fallback_stats_key(ref_);
		if let Some(key) = fallback_key {
			has_fallback_live_sessions = true;
			refresh_fallback(cache, key, ref_, refresh_fallback_reads);
		}
		let live = fallback_key.and_then(|k| cache.entries.get(&k)).and_then(|e| e.metrics.clone());
		let metrics = project_row_metrics(
			ref_.history().as_ref(),
			observed.as_ref(),
			live.as_ref(),
			ref_.retained_metrics().as_ref(),
			ref_.has_live_session(),
		);
		if let Some(key) = fallback_key {
			if counted_fallback_sessions.contains(&key) {
				continue;
			}
		}
		let Some(metrics) = metrics else { continue };
		if let Some(key) = fallback_key {
			counted_fallback_sessions.insert(key);
		}
		total.reported_agents += 1;
		total.tokens += metrics.tokens;
		total.requests += metrics.requests;
		total.tools += metrics.tools;
		total.cost += metrics.cost;
		if metrics.duration_kind == "active" {
			total.duration_ms += metrics.duration_ms;
			total.active_duration_agents += 1;
		}
	}
	(total, has_fallback_live_sessions)
}

/// Parent-before-child projection preserving initial subtree rank and prepending new siblings.
#[must_use]
pub fn project_agent_tree<T, F>(refs: Vec<T>, spawned_while_open: F) -> AgentTreeProjection<T>
where
	T: AgentRecordView + Clone,
	F: Fn(&T) -> bool,
{
	let mut ids: HashSet<String> = HashSet::new();
	let mut operational_index: HashMap<String, usize> = HashMap::new();
	for (i, r) in refs.iter().enumerate() {
		ids.insert(r.id().to_string());
		operational_index.insert(r.id().to_string(), i);
	}

	let mut parent_by_id: HashMap<String, String> = HashMap::new();
	let mut children: HashMap<String, Vec<T>> = HashMap::new();
	for r in &refs {
		let parent = match r.parent_id() {
			Some(pid) if pid != MAIN_AGENT_ID && ids.contains(pid) => pid.to_string(),
			_ => MAIN_AGENT_ID.to_string(),
		};
		parent_by_id.insert(r.id().to_string(), parent.clone());
		children.entry(parent).or_default().push(r.clone());
	}

	let mut subtree_order: HashMap<String, usize> = HashMap::new();
	let mut visiting: HashSet<String> = HashSet::new();
	let mut ranked: HashSet<String> = HashSet::new();
	#[derive(Clone)]
	struct StackItem<T> {
		ref_: T,
		expanded: bool,
	}
	for start in &refs {
		if ranked.contains(start.id()) {
			continue;
		}
		let mut stack = vec![StackItem {
			ref_: start.clone(),
			expanded: false,
		}];
		while let Some(current) = stack.pop() {
			if current.expanded {
				let own = *operational_index
					.get(current.ref_.id())
					.unwrap_or(&usize::MAX);
				let mut order = own;
				if let Some(desc) = children.get(current.ref_.id()) {
					for child in desc {
						let child_idx = *operational_index.get(child.id()).unwrap_or(&usize::MAX);
						if spawned_while_open(child) && child_idx < own {
							continue;
						}
						order = order.min(*subtree_order.get(child.id()).unwrap_or(&usize::MAX));
					}
				}
				subtree_order.insert(current.ref_.id().to_string(), order);
				visiting.remove(current.ref_.id());
				ranked.insert(current.ref_.id().to_string());
				continue;
			}
			if ranked.contains(current.ref_.id()) || visiting.contains(current.ref_.id()) {
				continue;
			}
			visiting.insert(current.ref_.id().to_string());
			stack.push(StackItem {
				ref_: current.ref_.clone(),
				expanded: true,
			});
			if let Some(desc) = children.get(current.ref_.id()) {
				for child in desc.iter().rev() {
					if !ranked.contains(child.id()) && !visiting.contains(child.id()) {
						stack.push(StackItem {
							ref_: child.clone(),
							expanded: false,
						});
					}
				}
			}
		}
	}
	for siblings in children.values_mut() {
		siblings.sort_by(|a, b| {
			let ao = *subtree_order.get(a.id()).unwrap_or(&usize::MAX);
			let bo = *subtree_order.get(b.id()).unwrap_or(&usize::MAX);
			ao.cmp(&bo).then_with(|| {
				operational_index
					.get(a.id())
					.copied()
					.unwrap_or(usize::MAX)
					.cmp(&operational_index.get(b.id()).copied().unwrap_or(usize::MAX))
			})
		});
	}
	let mut last_sibling_by_id: HashMap<String, bool> = HashMap::new();
	for siblings in children.values() {
		for (i, s) in siblings.iter().enumerate() {
			last_sibling_by_id.insert(s.id().to_string(), i + 1 == siblings.len());
		}
	}

	let mut rows: Vec<T> = Vec::new();
	let mut visited: HashSet<String> = HashSet::new();
	let mut depth_by_id: HashMap<String, usize> = HashMap::new();
	fn visit<T: AgentRecordView + Clone>(
		root: T,
		root_depth: usize,
		children: &HashMap<String, Vec<T>>,
		rows: &mut Vec<T>,
		visited: &mut HashSet<String>,
		depth_by_id: &mut HashMap<String, usize>,
	) {
		let mut stack = vec![(root, root_depth)];
		while let Some((ref_, depth)) = stack.pop() {
			if visited.contains(ref_.id()) {
				continue;
			}
			visited.insert(ref_.id().to_string());
			depth_by_id.insert(ref_.id().to_string(), depth);
			rows.push(ref_.clone());
			if let Some(desc) = children.get(ref_.id()) {
				for child in desc.iter().rev() {
					stack.push((child.clone(), depth + 1));
				}
			}
		}
	}
	if let Some(roots) = children.get(MAIN_AGENT_ID) {
		for root in roots.clone() {
			visit(
				root,
				0,
				&children,
				&mut rows,
				&mut visited,
				&mut depth_by_id,
			);
		}
	}
	for ref_ in &refs {
		if !visited.contains(ref_.id()) {
			visit(
				ref_.clone(),
				0,
				&children,
				&mut rows,
				&mut visited,
				&mut depth_by_id,
			);
		}
	}
	AgentTreeProjection {
		rows,
		depth_by_id,
		parent_by_id,
		last_sibling_by_id,
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[derive(Clone, Debug)]
	struct Rec {
		id: String,
		parent: Option<String>,
		status: AgentStatus,
		created: i64,
		last: i64,
		history: Option<AgentRecordHistoryView>,
	}
	impl AgentRecordView for Rec {
		fn id(&self) -> &str {
			&self.id
		}
		fn parent_id(&self) -> Option<&str> {
			self.parent.as_deref()
		}
		fn status(&self) -> AgentStatus {
			self.status
		}
		fn created_at(&self) -> i64 {
			self.created
		}
		fn last_activity(&self) -> i64 {
			self.last
		}
		fn history(&self) -> Option<AgentRecordHistoryView> {
			self.history.clone()
		}
	}

	#[test]
	fn progress_metrics_requires_complete_finite_snapshot() {
		assert!(progress_metrics(None).is_none());
		let p = ObservableSessionView {
			progress: Some(ObservedProgress {
				tokens: Some(10.0),
				requests: Some(2.0),
				tool_count: Some(3.0),
				cost: Some(0.5),
				duration_ms: Some(100.0),
				context_tokens: Some(5.0),
				context_window: Some(20.0),
			}),
		};
		let m = progress_metrics(Some(&p)).expect("metrics");
		assert_eq!(m.tokens, 10);
		assert_eq!(m.cost, 0.5);
		assert_eq!(m.duration_kind, "active");
		let bad = ObservableSessionView {
			progress: Some(ObservedProgress {
				tokens: None,
				requests: Some(1.0),
				tool_count: Some(1.0),
				cost: Some(0.0),
				duration_ms: Some(1.0),
				..Default::default()
			}),
		};
		assert!(progress_metrics(Some(&bad)).is_none());
	}

	#[test]
	fn project_row_metrics_overlays_cost_on_history_metrics() {
		let history = AgentRecordHistoryView {
			metrics: Some(AgentMetrics {
				tokens: 7,
				requests: 1,
				tools: 2,
				cost: 0.0,
				duration_ms: 5,
				duration_kind: "span",
				context_tokens: None,
				context_window: None,
			}),
			direct_cost: Some(1.25),
			..Default::default()
		};
		let m = project_row_metrics(Some(&history), None, None, None, false).expect("metrics");
		assert_eq!(m.tokens, 7);
		assert_eq!(m.cost, 1.25);
	}

	#[test]
	fn project_agent_tree_is_parent_before_child_and_marks_last_sibling() {
		let refs = vec![
			Rec { id: "a".into(), parent: None, status: AgentStatus::Running, created: 1, last: 1, history: None },
			Rec { id: "b".into(), parent: Some("a".into()), status: AgentStatus::Idle, created: 2, last: 2, history: None },
			Rec { id: "c".into(), parent: Some("a".into()), status: AgentStatus::Idle, created: 3, last: 3, history: None },
		];
		let proj = project_agent_tree(refs, |_| false);
		assert_eq!(proj.rows.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(), vec!["a", "b", "c"]);
		assert_eq!(proj.depth_by_id.get("a"), Some(&0));
		assert_eq!(proj.depth_by_id.get("b"), Some(&1));
		assert_eq!(proj.last_sibling_by_id.get("b"), Some(&false));
		assert_eq!(proj.last_sibling_by_id.get("c"), Some(&true));
	}
}
