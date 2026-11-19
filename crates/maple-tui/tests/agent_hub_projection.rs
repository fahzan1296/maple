//! Port of the pure-projection cases in `l/test/agent-hub-projection.test.ts`.
//!
//! Deviations: the reference's JS records expose `session.agent.state.messages`,
//! `getSessionStats`, and `sessionManager.getUsageStatistics`; the crate reads
//! fallback usage through the [`AgentRecordView`] trait, so [`Record`] below
//! adapts an in-memory transcript. The indexed-bill/SDK-fallback case has no
//! trait surface and stays ignored.

use std::cell::{Cell, RefCell};

use maple_tui::overlays::agent_hub_projection::{
	AgentMetrics, AgentRecordHistoryView, AgentRecordView, FallbackStamp, ObservableSessionView,
	ObservedProgress, ProjectionCache, aggregate_cost, aggregate_metrics, hub_row_metrics,
};
use maple_tui::overlays::agent_hub_types::AgentStatus;

struct Msg {
	role: &'static str,
	input: u64,
	output: u64,
	cost: f64,
}

fn assistant(output: u64, cost: f64) -> Msg {
	Msg { role: "assistant", input: 10, output, cost }
}

struct Record {
	id: &'static str,
	history_metrics: Option<AgentMetrics>,
	direct_cost: Option<f64>,
	fallback_key: Option<usize>,
	session_present: Cell<bool>,
	messages: RefCell<Vec<Msg>>,
	cached: RefCell<Option<AgentMetrics>>,
}

impl Record {
	fn new(id: &'static str, messages: Vec<Msg>) -> Self {
		Record {
			id,
			history_metrics: None,
			direct_cost: None,
			fallback_key: None,
			session_present: Cell::new(!messages.is_empty()),
			messages: RefCell::new(messages),
			cached: RefCell::new(None),
		}
	}

	fn retained(id: &'static str, metrics: AgentMetrics, direct_cost: Option<f64>) -> Self {
		Record {
			id,
			history_metrics: Some(metrics),
			direct_cost,
			fallback_key: None,
			session_present: Cell::new(false),
			messages: RefCell::new(Vec::new()),
			cached: RefCell::new(None),
		}
	}
}

impl AgentRecordView for Record {
	fn id(&self) -> &str {
		self.id
	}
	fn parent_id(&self) -> Option<&str> {
		None
	}
	fn status(&self) -> AgentStatus {
		AgentStatus::Idle
	}
	fn created_at(&self) -> i64 {
		0
	}
	fn last_activity(&self) -> i64 {
		0
	}
	fn history(&self) -> Option<AgentRecordHistoryView> {
		Some(AgentRecordHistoryView {
			metrics: self.history_metrics.clone(),
			direct_cost: self.direct_cost,
			resolved_model: None,
			resolved_model_is_fallback: None,
		})
	}
	fn has_live_session(&self) -> bool {
		self.session_present.get()
	}
	fn fallback_cache_key(&self) -> Option<usize> {
		self.fallback_key
	}
	fn fallback_live_metrics(&self) -> Option<AgentMetrics> {
		if !self.session_present.get() {
			// A parked record keeps the last successful fallback read.
			return self.cached.borrow().clone();
		}
		let messages = self.messages.borrow();
		if messages.is_empty() {
			return None;
		}
		let mut tokens = 0_u64;
		let mut requests = 0_u64;
		let mut cost = 0.0_f64;
		for message in messages.iter().filter(|message| message.role == "assistant") {
			tokens += message.input + message.output;
			requests += 1;
			cost += message.cost;
		}
		let metrics = AgentMetrics {
			tokens,
			requests,
			tools: 0,
			cost,
			duration_ms: 0,
			duration_kind: "span",
			context_tokens: None,
			context_window: None,
		};
		*self.cached.borrow_mut() = Some(metrics.clone());
		Some(metrics)
	}
	fn fallback_stamp(&self) -> FallbackStamp {
		let messages = self.messages.borrow();
		FallbackStamp {
			messages_len: messages.len(),
			last_shape: messages
				.last()
				.map(|message| format!("{}:{}", message.role, message.output))
				.unwrap_or_default(),
			model_key: String::new(),
			context_window: None,
		}
	}
}

fn observed(cost: f64) -> ObservableSessionView {
	ObservableSessionView {
		progress: Some(ObservedProgress {
			tokens: Some(80.0),
			requests: Some(4.0),
			tool_count: Some(3.0),
			cost: Some(cost),
			duration_ms: Some(800.0),
			context_tokens: Some(64.0),
			context_window: Some(2048.0),
		}),
	}
}

#[test]
fn refreshes_when_the_streaming_last_message_usage_changes_in_place() {
	let record = Record::new("main", vec![assistant(5, 0.005)]);
	record.fallback_key = Some(7);
	let mut cache = ProjectionCache::new();

	let totals = aggregate_metrics(std::slice::from_ref(&record), |_| None, &mut cache, true).0;
	assert_eq!(totals.tokens, 15);

	record.messages.borrow_mut()[0].output = 40;
	let totals = aggregate_metrics(std::slice::from_ref(&record), |_| None, &mut cache, true).0;
	assert_eq!(totals.tokens, 50);
}

#[test]
fn uses_cached_lifetime_session_cost_with_live_progress_and_refreshes_it() {
	let parent = Record::new("main", vec![assistant(100, 0.6), assistant(200, 0.3)]);
	parent.fallback_key = Some(7);
	let child = Record::new("child", vec![assistant(50, 0.2)]);
	child.fallback_key = Some(8);
	let rows = vec![parent, child];
	let observed_for = |id: &str| match id {
		"main" => Some(observed(0.3)),
		"child" => Some(observed(0.2)),
		_ => None,
	};
	let mut cache = ProjectionCache::new();

	let (first, _) = aggregate_metrics(&rows, observed_for, &mut cache, true);
	assert!((first.cost - 1.1).abs() < 1e-9, "cost {}", first.cost);
	assert_eq!(first.tokens, 160);
	assert_eq!(first.requests, 8);
	assert_eq!(first.tools, 6);
	assert_eq!(first.duration_ms, 1600);
	assert_eq!(first.duration_kind, "active");
	assert_eq!(first.active_duration_agents, 2);

	let first_row = hub_row_metrics(&rows[0], Some(&observed(0.3)), &mut cache, true).expect("row");
	assert!((first_row.cost - 0.9).abs() < 1e-9, "row cost {}", first_row.cost);
	assert_eq!(first_row.tokens, 80);
	assert_eq!(first_row.context_tokens, Some(64));
	assert_eq!(first_row.context_window, Some(2048));

	rows[0].messages.borrow_mut().push(assistant(300, 0.2));
	let (refreshed, _) = aggregate_metrics(&rows, observed_for, &mut cache, true);
	assert!((refreshed.cost - 1.3).abs() < 1e-9, "refreshed cost {}", refreshed.cost);
	let refreshed_row = hub_row_metrics(&rows[0], Some(&observed(0.2)), &mut cache, true).expect("row");
	assert!((refreshed_row.cost - 1.1).abs() < 1e-9, "row cost {}", refreshed_row.cost);

	// Parking the session keeps the last successful fallback read.
	rows[0].session_present.set(false);
	let (parked, _) = aggregate_metrics(&rows, observed_for, &mut cache, true);
	assert!((parked.cost - 1.3).abs() < 1e-9, "parked cost {}", parked.cost);
	assert_eq!(parked.reported_agents, 2);
	let parked_row = hub_row_metrics(&rows[0], Some(&observed(0.2)), &mut cache, true).expect("row");
	assert!((parked_row.cost - 1.1).abs() < 1e-9, "parked row cost {}", parked_row.cost);
}

#[test]
fn excludes_nested_task_result_usage_while_counting_the_child_row_once() {
	let mut parent = Record::new(
		"parent",
		vec![
			assistant(100, 0.6),
			Msg { role: "toolCall", input: 0, output: 0, cost: 0.0 },
			Msg { role: "toolResult", input: 0, output: 0, cost: 99.0 },
		],
	);
	parent.fallback_key = Some(7);
	let mut child = Record::new("child", vec![assistant(20, 0.2)]);
	child.fallback_key = Some(8);
	let rows = vec![parent, child];
	let observed_for = |id: &str| match id {
		"parent" => Some(observed(0.6)),
		"child" => Some(observed(0.2)),
		_ => None,
	};
	let mut cache = ProjectionCache::new();

	let (metrics, _) = aggregate_metrics(&rows, observed_for, &mut cache, true);
	assert!((metrics.cost - 0.8).abs() < 1e-9, "cost {}", metrics.cost);
	assert_eq!(metrics.reported_agents, 2);
}

#[test]
fn does_not_invent_a_parked_row_from_direct_cost_history_without_a_retained_snapshot() {
	let metrics = AgentMetrics {
		tokens: 1,
		requests: 1,
		tools: 0,
		cost: 0.4,
		duration_ms: 0,
		duration_kind: "span",
		context_tokens: None,
		context_window: None,
	};
	let mut record = Record::retained("worker", metrics.clone(), Some(2.0));
	record.history_metrics = None;
	let mut cache = ProjectionCache::new();
	assert_eq!(aggregate_cost(std::slice::from_ref(&record), |_| None, &mut cache), 0.0);
	// A retained snapshot makes the row real; its cost comes from the direct cost.
	record.history_metrics = Some(metrics);
	assert_eq!(aggregate_cost(std::slice::from_ref(&record), |_| None, &mut cache), 2.0);
	record.direct_cost = None;
	let cost = aggregate_cost(std::slice::from_ref(&record), |_| None, &mut cache);
	assert!((cost - 0.4).abs() < 1e-9, "cost {cost}");
}

#[test]
fn preserves_observer_validity_and_history_precedence_without_counting_shared_sessions_twice() {
	let mut record = Record::retained(
		"worker",
		AgentMetrics {
			tokens: 1,
			requests: 1,
			tools: 0,
			cost: 0.1,
			duration_ms: 0,
			duration_kind: "span",
			context_tokens: None,
			context_window: None,
		},
		None,
	);
	record.fallback_key = Some(7);
	let mut cache = ProjectionCache::new();
	let rows = vec![record];

	let cost = aggregate_cost(&rows, |_| Some(observed(0.2)), &mut cache);
	assert!((cost - 0.2).abs() < 1e-9, "observer cost {cost}");

	rows[0].direct_cost = Some(0.4);
	let mut cache = ProjectionCache::new();
	let cost = aggregate_cost(&rows, |_| Some(observed(0.2)), &mut cache);
	assert!((cost - 0.4).abs() < 1e-9, "history precedence {cost}");

	// An alias row sharing the fallback session key is counted once.
	let mut alias = rows[0].clone_for_test();
	alias.id = "alias";
	let shared = vec![rows[0].clone_for_test(), alias];
	let mut cache = ProjectionCache::new();
	let cost = aggregate_cost(&shared, |_| Some(observed(0.2)), &mut cache);
	assert!((cost - 0.4).abs() < 1e-9, "shared session counted once: {cost}");

	// A non-finite observer token count invalidates the observer projection.
	let mut invalid = observed(0.2);
	invalid.progress.as_mut().expect("progress").tokens = Some(f64::NAN);
	let mut cache = ProjectionCache::new();
	let cost = aggregate_cost(&rows, |_| Some(invalid), &mut cache);
	assert_eq!(cost, 0.0, "invalid observer usage drops the row");
}

impl Record {
	fn clone_for_test(&self) -> Record {
		Record {
			id: self.id,
			history_metrics: self.history_metrics.clone(),
			direct_cost: self.direct_cost,
			fallback_key: self.fallback_key,
			session_present: Cell::new(self.session_present.get()),
			messages: RefCell::new(self.messages.borrow().clone()),
			cached: RefCell::new(self.cached.borrow().clone()),
		}
	}
}

#[test]
#[ignore = "missing facility: no sessionManager.getUsageStatistics adapter; SDK/indexed-bill fallback is host-provided"]
fn falls_back_to_direct_assistant_usage_when_the_indexed_bill_is_invalid() {
	let record = Record::new("worker", vec![assistant(100, 0.6)]);
	record.fallback_key = Some(7);
	let mut cache = ProjectionCache::new();
	let cost = aggregate_cost(std::slice::from_ref(&record), |_| None, &mut cache);
	assert!((cost - 0.6).abs() < 1e-9, "cost {cost}");
}
