//! Real `AgentHubOverlayComponent` port.
//!
//! Reference: `l/src/overlays/agent-hub.ts` (`AgentHubOverlayComponent`,
//! table/roster-tree/activity sections, chat/transcript activation, revive/kill,
//! filter/scope navigation, native picker-style describe).
//!
//! Deviations: TypeScript TUI widgets (`Container`, `SelectList`, `Input`,
//! `SplitPane` rendering stack, native picker integration, `SessionObserverRegistry`
//! tailing, `AgentTranscriptViewer` mounting, async focus/revive flows, and
//! registry subscription/clock ticking) are host-driven. This port owns
//! section/view/filter/scope/selection/activity state, row projection inputs,
//! kill/revive/activate/chat-open transitions, bordered render, keyboard/mouse
//! routing, and native tree/rows describe. Host integrations are injected via
//! traits/callbacks; singleton TUI/runtime services are not accessed directly.

use crate::chrome::overlay_box::{bottom_border, divider, row, top_border, OverlayTheme};
use crate::component::Component;
use crate::keys::{self, Key};
use crate::keybindings::KeybindingsManager;
use crate::mouse::{MouseEvent, MouseKind};
use crate::native::describe::NativeDescribe;
use crate::native::node::{DescribeContext, NativeNode, NodeKind, Props};
use crate::overlays::agent_activity::{activity_rows_from_progress, AgentActivityRow, AgentProgress};
use crate::overlays::agent_hub_projection::{AggregateMetrics, AgentMetrics};
use crate::overlays::agent_hub_renderer::{
	format_cost, format_model_badge, format_role_badge, metrics_text, sanitize_line, status_glyph,
	status_symbol, task_summary, AgentRoleDisplay,
};
use crate::overlays::agent_hub_types::{
	AgentHubRegistry, AgentHubSession, AgentLifecycleLike, AgentMetricsSummary, AgentRecordHistory,
	AgentRecordLifecycle, AgentRecordLike, AgentStatus, DurationKind, IrcBusLike, SessionStats,
	MAIN_AGENT_ID,
};
use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

pub const SPLIT_MIN_WIDTH: usize = 96;
pub const DETAIL_MIN_WIDTH: usize = 34;
pub const ROSTER_MIN_WIDTH: usize = 48;
pub const AGE_TICK_MS: u64 = 5_000;
pub const DATA_CHANGE_RENDER_COALESCE_MS: u64 = 100;
pub const NATIVE_RECENT_ACTIVITY: usize = 20;
pub const NATIVE_CHILD_IDS: usize = 12;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AgentHubSection {
	Agents,
	Activity,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum HubViewMode {
	Roster,
	Tree,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum StatusScope {
	All,
	Running,
	Done,
	Parked,
}

impl StatusScope {
	#[must_use]
	pub fn label(self) -> &'static str {
		match self {
			Self::All => "All",
			Self::Running => "Running",
			Self::Done => "Done",
			Self::Parked => "Parked",
		}
	}

	#[must_use]
	pub fn status(self) -> Option<AgentStatus> {
		match self {
			Self::All => None,
			Self::Running => Some(AgentStatus::Running),
			Self::Done => Some(AgentStatus::Idle),
			Self::Parked => Some(AgentStatus::Parked),
		}
	}
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ActivityFilter {
	All,
	Errors,
	Responses,
	Tools,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ActivityScope {
	All,
	Agent,
	Subtree,
}

#[derive(Clone, Debug, PartialEq)]
pub struct HubRow {
	pub id: String,
	pub display_name: String,
	pub kind: String,
	pub parent_id: Option<String>,
	pub status: AgentStatus,
	pub created_at: i64,
	pub last_activity: i64,
	pub activity: Option<String>,
	pub history: Option<AgentRecordHistory>,
	pub lifecycle: Option<AgentRecordLifecycle>,
	pub session_file: Option<String>,
}

impl HubRow {
	#[must_use]
	pub fn from_record<R: AgentRecordLike + ?Sized>(r: &R) -> Self {
		Self {
			id: r.id().to_string(),
			display_name: r.display_name().to_string(),
			kind: r.kind().to_string(),
			parent_id: r.parent_id().map(str::to_string),
			status: r.status(),
			created_at: r.created_at(),
			last_activity: r.last_activity(),
			activity: r.activity().map(str::to_string),
			history: r.history(),
			lifecycle: r.lifecycle(),
			session_file: r.session_file().map(str::to_string),
		}
	}
}

fn duration_kind_token(kind: Option<DurationKind>) -> &'static str {
	match kind {
		Some(DurationKind::Active) => "active",
		Some(DurationKind::Span) => "span",
		_ => "unknown",
	}
}

fn metrics_from_history(history: Option<&AgentRecordHistory>) -> Option<AgentMetrics> {
	let summary: AgentMetricsSummary = history?.metrics.clone()?;
	Some(AgentMetrics {
		tokens: summary.tokens,
		requests: summary.requests,
		tools: summary.tools,
		cost: summary.cost,
		duration_ms: summary.duration_ms,
		duration_kind: duration_kind_token(summary.duration_kind),
		context_tokens: summary.context_tokens,
		context_window: summary.context_window,
	})
}

fn metrics_from_stats(history: Option<&AgentRecordHistory>, stats: &SessionStats) -> AgentMetrics {
	let mut m = metrics_from_history(history).unwrap_or_default();
	m.tokens = stats.tokens.input.saturating_add(stats.tokens.output);
	m.requests = stats.assistant_messages;
	m.tools = stats.tool_calls;
	m.cost = stats.cost;
	m.duration_kind = "unknown";
	if let Some(cu) = &stats.context_usage {
		m.context_tokens = Some(cu.tokens);
		m.context_window = Some(cu.context_window);
	}
	m
}

/// Host callbacks for hub actions that depend on runtime singletons/services.
pub trait AgentHubCallbacks {
	fn on_done(&self);
	fn request_render(&self);
	/// Focus main view on agent live session (host ensureLive+switch); return Err to show notice.
	fn focus_agent(&self, _id: &str) -> Result<(), String> {
		Err("focus unavailable".into())
	}
}

/// Remote hub adapter for collab/guest contexts.
pub trait AgentHubRemote {
	fn kill(&self, id: &str);
	fn revive(&self, id: &str);
}

/// Snapshot/cache of one row's render metrics.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RowCache {
	pub metrics: Option<AgentMetrics>,
	pub unread: usize,
	pub summary: String,
	pub metrics_line: String,
}

/// Agent hub overlay component.
///
/// Generic over registry/lifecycle/irc abstractions so host can inject
/// concrete implementations (or test doubles) without TUI singletons.
pub struct AgentHubOverlayComponent<R, L, I, C>
where
	R: AgentHubRegistry + ?Sized,
	L: AgentLifecycleLike<R> + ?Sized,
	I: IrcBusLike + ?Sized,
	C: AgentHubCallbacks,
{
	registry: Arc<R>,
	lifecycle: Arc<L>,
	irc: Arc<I>,
	remote: Option<Arc<dyn AgentHubRemote + Send + Sync>>,
	callbacks: C,

	pub section: AgentHubSection,
	pub view_mode: HubViewMode,
	pub status_scope: StatusScope,
	pub activity_filter: ActivityFilter,
	pub activity_scope: ActivityScope,
	pub filter_text: String,
	pub filter_mode: bool,
	pub rows: Vec<HubRow>,
	pub selected_row: usize,
	pub selected_id: Option<String>,
	pub chat_agent_id: Option<String>,
	pub notice: Option<String>,
	pub activity_rows: Vec<AgentActivityRow>,
	pub activity_selected: usize,
	pub follow_activity: bool,
	row_cache: HashMap<String, RowCache>,
	last_left_tap_ms: i64,
	now_ms: i64,

	bindings: KeybindingsManager,
	theme: OverlayTheme,
}

impl<R, L, I, C> fmt::Debug for AgentHubOverlayComponent<R, L, I, C>
where
	R: AgentHubRegistry + ?Sized,
	L: AgentLifecycleLike<R> + ?Sized,
	I: IrcBusLike + ?Sized,
	C: AgentHubCallbacks,
{
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.debug_struct("AgentHubOverlayComponent")
			.field("section", &self.section)
			.field("view_mode", &self.view_mode)
			.field("rows", &self.rows.len())
			.field("selected_row", &self.selected_row)
			.field("chat_agent_id", &self.chat_agent_id)
			.finish_non_exhaustive()
	}
}

impl<R, L, I, C> AgentHubOverlayComponent<R, L, I, C>
where
	R: AgentHubRegistry + ?Sized,
	L: AgentLifecycleLike<R> + ?Sized,
	I: IrcBusLike + ?Sized,
	C: AgentHubCallbacks,
{
	pub fn new(registry: Arc<R>, lifecycle: Arc<L>, irc: Arc<I>, callbacks: C) -> Self {
		let mut this = Self {
			registry,
			lifecycle,
			irc,
			remote: None,
			callbacks,
			section: AgentHubSection::Agents,
			view_mode: HubViewMode::Roster,
			status_scope: StatusScope::All,
			activity_filter: ActivityFilter::All,
			activity_scope: ActivityScope::All,
			filter_text: String::new(),
			filter_mode: false,
			rows: Vec::new(),
			selected_row: 0,
			selected_id: None,
			chat_agent_id: None,
			notice: None,
			activity_rows: Vec::new(),
			activity_selected: 0,
			follow_activity: false,
			row_cache: HashMap::new(),
			last_left_tap_ms: 0,
			now_ms: 0,
			bindings: KeybindingsManager::new(),
			theme: OverlayTheme::default(),
		};
		this.refresh_rows();
		this
	}

	pub fn set_remote(&mut self, remote: Option<Arc<dyn AgentHubRemote + Send + Sync>>) {
		self.remote = remote;
	}

	pub fn set_keybindings(&mut self, bindings: KeybindingsManager) {
		self.bindings = bindings;
	}

	pub fn set_theme(&mut self, theme: OverlayTheme) {
		self.theme = theme;
	}

	pub fn set_now_ms(&mut self, now_ms: i64) {
		self.now_ms = now_ms;
	}

	pub fn refresh_rows(&mut self) {
		let records = self.registry.list();
		let mut rows: Vec<HubRow> = records
			.iter()
			.filter(|r| r.id() != MAIN_AGENT_ID)
			.map(|r| HubRow::from_record(r.as_ref()))
			.collect();
		if let Some(status) = self.status_scope.status() {
			rows.retain(|r| r.status == status);
		}
		if !self.filter_text.trim().is_empty() {
			let needle = self.filter_text.to_ascii_lowercase();
			rows.retain(|r| {
				r.id.to_ascii_lowercase().contains(&needle)
					|| r.display_name.to_ascii_lowercase().contains(&needle)
					|| r.activity.as_deref().unwrap_or_default().to_ascii_lowercase().contains(&needle)
			});
		}
		rows.sort_by(|a, b| {
			b.last_activity
				.cmp(&a.last_activity)
				.then_with(|| a.id.cmp(&b.id))
		});
		self.rows = rows;
		self.rebuild_cache();
		self.restore_selection();
	}

	fn rebuild_cache(&mut self) {
		self.row_cache.clear();
		for r in &self.rows {
			let session = self.registry.get(&r.id).and_then(|rec| rec.session());
			let metrics = match &session {
				Some(s) => Some(metrics_from_stats(r.history.as_ref(), &s.get_session_stats())),
				None => metrics_from_history(r.history.as_ref()),
			};
			let unread = self.irc.unread_count(&r.id);
			let summary = task_summary(r.activity.as_deref().unwrap_or_default());
			let metrics_line = metrics.as_ref().map(metrics_text).unwrap_or_default();
			self.row_cache.insert(
				r.id.clone(),
				RowCache {
					metrics,
					unread,
					summary,
					metrics_line,
				},
			);
		}
	}

	fn restore_selection(&mut self) {
		if let Some(id) = &self.selected_id {
			if let Some(idx) = self.rows.iter().position(|r| &r.id == id) {
				self.selected_row = idx;
				return;
			}
		}
		if self.rows.is_empty() {
			self.selected_row = 0;
			self.selected_id = None;
		} else {
			self.selected_row = self.selected_row.min(self.rows.len() - 1);
			self.selected_id = self.rows.get(self.selected_row).map(|r| r.id.clone());
		}
	}

	fn select_row(&mut self, idx: usize) {
		if self.rows.is_empty() {
			self.selected_row = 0;
			self.selected_id = None;
			return;
		}
		self.selected_row = idx.min(self.rows.len() - 1);
		self.selected_id = self.rows.get(self.selected_row).map(|r| r.id.clone());
	}

	fn selected(&self) -> Option<&HubRow> {
		self.rows.get(self.selected_row)
	}

	fn aggregate(&self) -> AggregateMetrics {
		let mut agg = AggregateMetrics { duration_kind: "active", ..Default::default() };
		for c in self.row_cache.values() {
			let Some(m) = &c.metrics else { continue };
			agg.reported_agents += 1;
			agg.tokens += m.tokens;
			agg.requests += m.requests;
			agg.tools += m.tools;
			agg.cost += m.cost;
			if m.duration_kind == "active" {
				agg.duration_ms += m.duration_ms;
				agg.active_duration_agents += 1;
			}
		}
		agg
	}

	pub fn open_chat(&mut self, id: impl Into<String>) {
		self.chat_agent_id = Some(id.into());
		self.callbacks.request_render();
	}

	pub fn close_chat(&mut self) {
		self.chat_agent_id = None;
		self.callbacks.request_render();
	}

	fn activate_selected(&mut self) {
		let Some(row) = self.selected().cloned() else {
			return;
		};
		if row.kind == "advisor" || self.remote.is_some() {
			self.open_chat(row.id);
			return;
		}
		match self.callbacks.focus_agent(&row.id) {
			Ok(()) => self.callbacks.on_done(),
			Err(e) => self.notice = Some(e),
		}
	}

	fn revive_selected(&mut self) {
		let Some(row) = self.selected().cloned() else {
			return;
		};
		if row.kind == "advisor" {
			self.notice = Some(format!("\"{}\" is a read-only advisor transcript — nothing to revive.", row.id));
			return;
		}
		if !matches!(row.status, AgentStatus::Parked | AgentStatus::Aborted) {
			self.notice = Some(format!("Agent \"{}\" is {:?} — only parked or killed agents can be revived.", row.id, row.status));
			return;
		}
		if let Some(remote) = &self.remote {
			remote.revive(&row.id);
			return;
		}
		match self.lifecycle.ensure_live(&row.id) {
			Ok(_) => self.notice = Some(format!("Revived {}", row.id)),
			Err(e) => self.notice = Some(e),
		}
	}

	fn kill_selected(&mut self) {
		let Some(row) = self.selected().cloned() else {
			return;
		};
		if row.kind == "advisor" {
			self.notice = Some(format!("\"{}\" is a read-only advisor transcript — cannot be killed.", row.id));
			return;
		}
		if let Some(remote) = &self.remote {
			remote.kill(&row.id);
			return;
		}
		if let Some(rec) = self.registry.get(&row.id) {
			if row.status == AgentStatus::Running {
				if let Some(session) = rec.session() {
					let _ = session.abort(Some("user interrupt"));
				}
			}
			match self.lifecycle.release(&row.id, rec.as_ref(), true) {
				Ok(_) => self.notice = Some(format!("Killed {}", row.id)),
				Err(e) => self.notice = Some(e),
			}
		}
		self.refresh_rows();
	}

	fn render_agent_rows(&self, inner_w: usize) -> Vec<String> {
		let mut out = Vec::new();
		if self.rows.is_empty() {
			out.push("(no agents)".to_string());
			return out;
		}
		for (i, r) in self.rows.iter().enumerate() {
			let cursor = if i == self.selected_row { "›" } else { " " };
			let cache = self.row_cache.get(&r.id);
			let unread = cache.map(|c| c.unread).unwrap_or(0);
			let summary = cache.map(|c| c.summary.as_str()).unwrap_or("");
			let unread_s = if unread > 0 { format!(" [{unread}]") } else { String::new() };
			out.push(sanitize_line(
				&format!(
					"{cursor} {} {} {}{}  {}",
					status_glyph(r.status),
					r.id,
					format_role_badge(&r.kind, &AgentRoleDisplay::default()),
					unread_s,
					summary,
				),
				Some(inner_w),
			));
		}
		out
	}

	fn render_detail(&self, inner_w: usize) -> Vec<String> {
		let Some(r) = self.selected() else {
			return vec!["No selection".to_string()];
		};
		let cache = self.row_cache.get(&r.id);
		let mut lines = vec![
			sanitize_line(&format!("{} {} ({})", status_symbol(r.status), r.id, r.kind), Some(inner_w)),
			sanitize_line(&format!("Name: {}", r.display_name), Some(inner_w)),
		];
		if let Some(parent) = &r.parent_id {
			lines.push(sanitize_line(&format!("Parent: {parent}"), Some(inner_w)));
		}
		if let Some(c) = cache {
			lines.push(sanitize_line(&format!("Task: {}", c.summary), Some(inner_w)));
			if !c.metrics_line.is_empty() {
				lines.push(sanitize_line(&c.metrics_line, Some(inner_w)));
			}
		}
		if let Some(h) = &r.history {
			if let Some(model) = &h.resolved_model {
				lines.push(sanitize_line(&format!("Model: {}", format_model_badge(model, None)), Some(inner_w)));
			}
		}
		if let Some(act) = &r.activity {
			lines.push(sanitize_line(&format!("Activity: {act}"), Some(inner_w)));
		}
		lines
	}

	fn render_frame(&self, width: u16) -> Vec<String> {
		let inner_w = width.saturating_sub(4) as usize;
		let agg = self.aggregate();
		let title = match self.section {
			AgentHubSection::Agents => format!("Agents · {:?} · {:?}", self.view_mode, self.status_scope),
			AgentHubSection::Activity => "Agent Activity".to_string(),
		};
		let mut out = vec![top_border(width, &title, &self.theme)];
		out.push(row(
			&sanitize_line(
				&format!(
					"Agents: {}  Running: {}  Cost: {}  Tokens: {}",
					agg.reported_agents,
					self.rows.iter().filter(|r| r.status == AgentStatus::Running).count(),
					format_cost(agg.cost),
					agg.tokens
				),
				Some(inner_w.max(10)),
			),
			width,
			&self.theme,
		));
		if !self.filter_text.is_empty() || self.filter_mode {
			out.push(row(&sanitize_line(&format!("/ {}", self.filter_text), Some(inner_w.max(10))), width, &self.theme));
		}
		out.push(divider(width, &self.theme));
		match self.section {
			AgentHubSection::Agents => {
				if width as usize >= SPLIT_MIN_WIDTH {
					let left_w = (inner_w * 3 / 5).max(20);
					let right_w = inner_w.saturating_sub(left_w + 3).max(20);
					let left = self.render_agent_rows(left_w);
					let right = self.render_detail(right_w);
					let n = left.len().max(right.len()).min(12);
					for i in 0..n {
						let l = left.get(i).cloned().unwrap_or_default();
						let r = right.get(i).cloned().unwrap_or_default();
						out.push(row(&format!("{:<lw$} │ {}", sanitize_line(&l, Some(left_w)), sanitize_line(&r, Some(right_w)), lw = left_w), width, &self.theme));
					}
				} else {
					for l in self.render_agent_rows(inner_w.max(10)).into_iter().take(12) {
						out.push(row(&l, width, &self.theme));
					}
				}
			}
			AgentHubSection::Activity => {
				if self.activity_rows.is_empty() {
					out.push(row("(no activity)", width, &self.theme));
				} else {
					for (i, ar) in self.activity_rows.iter().enumerate().take(12) {
						let cursor = if i == self.activity_selected { "›" } else { " " };
						out.push(row(&sanitize_line(&format!("{cursor} {} {}", ar.source, ar.title), Some(inner_w.max(10))), width, &self.theme));
					}
				}
			}
		}
		out.push(divider(width, &self.theme));
		let hint = self.notice.clone().unwrap_or_else(|| match self.section {
			AgentHubSection::Agents => "↑/↓ select · enter open/chat · t view · tab scope · / filter · r revive · x kill · 2 activity · esc close".to_string(),
			AgentHubSection::Activity => "↑/↓ select · space follow · f filter · s scope · 1 agents · esc close".to_string(),
		});
		out.push(row(&sanitize_line(&hint, Some(inner_w.max(10))), width, &self.theme));
		out.push(bottom_border(width, &self.theme));
		out
	}

	fn cycle_status_scope(&mut self) {
		self.status_scope = match self.status_scope {
			StatusScope::All => StatusScope::Running,
			StatusScope::Running => StatusScope::Done,
			StatusScope::Done => StatusScope::Parked,
			StatusScope::Parked => StatusScope::All,
		};
		self.refresh_rows();
	}
}

impl<R, L, I, C> Component for AgentHubOverlayComponent<R, L, I, C>
where
	R: AgentHubRegistry + ?Sized,
	L: AgentLifecycleLike<R> + ?Sized,
	I: IrcBusLike + ?Sized,
	C: AgentHubCallbacks,
{
	fn render(&mut self, width: u16) -> Vec<String> {
		self.render_frame(width)
	}

	fn handle_input(&mut self, key: &Key) {
		self.notice = None;
		if self.filter_mode {
			match key {
				Key::Esc => {
					self.filter_mode = false;
					self.filter_text.clear();
					self.refresh_rows();
				}
				Key::Enter => {
					self.filter_mode = false;
				}
				Key::Backspace => {
					self.filter_text.pop();
					self.refresh_rows();
				}
				Key::Char(c) => {
					self.filter_text.push(*c);
					self.refresh_rows();
				}
				_ => {}
			}
			self.callbacks.request_render();
			return;
		}

		match self.section {
			AgentHubSection::Agents => match key {
				Key::Esc => self.callbacks.on_done(),
				Key::Up | Key::Char('k') => {
					self.select_row(self.selected_row.saturating_sub(1));
					self.callbacks.request_render();
				}
				Key::Down | Key::Char('j') => {
					self.select_row(self.selected_row.saturating_add(1));
					self.callbacks.request_render();
				}
				Key::Enter => self.activate_selected(),
				Key::Char('t') => {
					self.view_mode = match self.view_mode {
						HubViewMode::Roster => HubViewMode::Tree,
						HubViewMode::Tree => HubViewMode::Roster,
					};
					self.callbacks.request_render();
				}
				Key::Char('/') => {
					self.filter_mode = true;
					self.callbacks.request_render();
				}
				Key::Tab => {
					self.cycle_status_scope();
					self.callbacks.request_render();
				}
				Key::Char('r') => {
					self.revive_selected();
					self.callbacks.request_render();
				}
				Key::Char('x') => {
					self.kill_selected();
					self.callbacks.request_render();
				}
				Key::Char('2') => {
					self.section = AgentHubSection::Activity;
					self.callbacks.request_render();
				}
				_ => {}
			},
			AgentHubSection::Activity => match key {
				Key::Esc => self.callbacks.on_done(),
				Key::Up | Key::Char('k') => {
					self.activity_selected = self.activity_selected.saturating_sub(1);
					self.callbacks.request_render();
				}
				Key::Down | Key::Char('j') => {
					if !self.activity_rows.is_empty() {
						self.activity_selected = (self.activity_selected + 1).min(self.activity_rows.len() - 1);
					}
					self.callbacks.request_render();
				}
				Key::Char(' ') => {
					self.follow_activity = !self.follow_activity;
					self.callbacks.request_render();
				}
				Key::Char('f') => {
					self.activity_filter = match self.activity_filter {
						ActivityFilter::All => ActivityFilter::Errors,
						ActivityFilter::Errors => ActivityFilter::Responses,
						ActivityFilter::Responses => ActivityFilter::Tools,
						ActivityFilter::Tools => ActivityFilter::All,
					};
					self.callbacks.request_render();
				}
				Key::Char('s') => {
					self.activity_scope = match self.activity_scope {
						ActivityScope::All => ActivityScope::Agent,
						ActivityScope::Agent => ActivityScope::Subtree,
						ActivityScope::Subtree => ActivityScope::All,
					};
					self.callbacks.request_render();
				}
				Key::Char('1') => {
					self.section = AgentHubSection::Agents;
					self.callbacks.request_render();
				}
				_ => {}
			},
		}
	}

	fn handle_mouse(&mut self, event: &MouseEvent) {
		match event.kind {
			MouseKind::ScrollUp => match self.section {
				AgentHubSection::Agents => self.select_row(self.selected_row.saturating_sub(1)),
				AgentHubSection::Activity => self.activity_selected = self.activity_selected.saturating_sub(1),
			},
			MouseKind::ScrollDown => match self.section {
				AgentHubSection::Agents => self.select_row(self.selected_row.saturating_add(1)),
				AgentHubSection::Activity => {
					if !self.activity_rows.is_empty() {
						self.activity_selected = (self.activity_selected + 1).min(self.activity_rows.len() - 1);
					}
				}
			},
			_ => {}
		}
		self.callbacks.request_render();
	}

	fn invalidate(&mut self) {
		self.refresh_rows();
	}
}

impl<R, L, I, C> NativeDescribe for AgentHubOverlayComponent<R, L, I, C>
where
	R: AgentHubRegistry + ?Sized,
	L: AgentLifecycleLike<R> + ?Sized,
	I: IrcBusLike + ?Sized,
	C: AgentHubCallbacks,
{
	fn describe_native(&self, cx: &DescribeContext) -> NativeNode {
		let lines = self.render_frame(cx.cols.max(40));
		let mut props = Props::new();
		props.set_i64("cols", i64::from(cx.cols.max(40)));
		props.insert("section", serde_json::Value::String(format!("{:?}", self.section)));
		props.insert("viewMode", serde_json::Value::String(format!("{:?}", self.view_mode)));
		props.insert("selectedId", serde_json::Value::String(self.selected_id.clone().unwrap_or_default()));
		props.insert(
			"lines",
			serde_json::Value::Array(lines.into_iter().map(serde_json::Value::String).collect()),
		);
		NativeNode::with_props(NodeKind::Rows, props)
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::cell::RefCell;
	use std::collections::BTreeMap;

	#[derive(Clone)]
	struct Rec {
		id: String,
		status: AgentStatus,
		kind: String,
		parent: Option<String>,
	}
	impl AgentRecordLike for Rec {
		fn id(&self) -> &str { &self.id }
		fn display_name(&self) -> &str { &self.id }
		fn kind(&self) -> &str { &self.kind }
		fn parent_id(&self) -> Option<&str> { self.parent.as_deref() }
		fn status(&self) -> AgentStatus { self.status }
		fn session(&self) -> Option<Arc<dyn AgentHubSession + Send + Sync>> { None }
		fn session_file(&self) -> Option<&str> { None }
		fn created_at(&self) -> i64 { 1 }
		fn last_activity(&self) -> i64 { 1 }
		fn activity(&self) -> Option<&str> { None }
		fn history(&self) -> Option<AgentRecordHistory> { None }
		fn lifecycle(&self) -> Option<AgentRecordLifecycle> { None }
	}

	struct Reg { list: Vec<Arc<Rec>> }
	impl AgentHubRegistry for Reg {
		type Record = Rec;
		fn list(&self) -> Vec<Arc<Self::Record>> { self.list.clone() }
		fn get(&self, id: &str) -> Option<Arc<Self::Record>> { self.list.iter().find(|r| r.id == id).cloned() }
		fn on_change(&self, _listener: Box<dyn Fn() + Send + Sync>) -> Box<dyn FnOnce() + Send> { Box::new(|| {}) }
	}

	struct Prompt;
	impl crate::overlays::agent_hub_types::LivePromptHandle for Prompt {
		fn prompt(&self, _text: &str, _streaming_behavior: &str) -> Result<(), String> { Ok(()) }
	}
	struct Life;
	impl AgentLifecycleLike<Reg> for Life {
		fn ensure_live(&self, _id: &str) -> Result<Arc<dyn crate::overlays::agent_hub_types::LivePromptHandle + Send + Sync>, String> { Ok(Arc::new(Prompt)) }
		fn release(&self, _id: &str, _expected: &Rec, _tombstone: bool) -> Result<bool, String> { Ok(true) }
	}
	struct Irc;
	impl IrcBusLike for Irc { fn unread_count(&self, _id: &str) -> usize { 0 } }
	struct Cb { done: RefCell<bool> }
	impl AgentHubCallbacks for Cb {
		fn on_done(&self) { *self.done.borrow_mut() = true; }
		fn request_render(&self) {}
	}

	#[test]
	fn agent_hub_component_renders_selects_and_cycles_scope() {
		let reg = Arc::new(Reg { list: vec![
			Arc::new(Rec { id: "a".into(), status: AgentStatus::Running, kind: "worker".into(), parent: None }),
			Arc::new(Rec { id: "b".into(), status: AgentStatus::Parked, kind: "worker".into(), parent: None }),
		]});
		let mut hub = AgentHubOverlayComponent::new(reg, Arc::new(Life), Arc::new(Irc), Cb { done: RefCell::new(false) });
		let lines = hub.render(100);
		assert!(lines.iter().any(|l| l.contains("Agents")));
		assert_eq!(hub.rows.len(), 2);
		hub.handle_input(&Key::Down);
		assert_eq!(hub.selected_row, 1);
		hub.handle_input(&Key::Tab); // Running scope
		assert!(hub.rows.iter().all(|r| r.status == AgentStatus::Running));
		let node = hub.describe_native(&DescribeContext { cols: 80, ..Default::default() });
		assert!(matches!(node.kind, NodeKind::Rows));
	}

	#[test]
	fn agent_hub_component_revive_kill_and_activity_section() {
		let reg = Arc::new(Reg { list: vec![
			Arc::new(Rec { id: "p".into(), status: AgentStatus::Parked, kind: "worker".into(), parent: None }),
		]});
		let mut hub = AgentHubOverlayComponent::new(reg, Arc::new(Life), Arc::new(Irc), Cb { done: RefCell::new(false) });
		hub.handle_input(&Key::Char('r'));
		assert!(hub.notice.as_deref().unwrap_or("").contains("Revived"));
		hub.refresh_rows();
		hub.handle_input(&Key::Char('2'));
		assert_eq!(hub.section, AgentHubSection::Activity);
		hub.handle_input(&Key::Char('f'));
		assert_eq!(hub.activity_filter, ActivityFilter::Errors);
	}
}
