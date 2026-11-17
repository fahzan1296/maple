//! Host and session contracts for the status line and footer.
//!
//! Reference: `l/src/status-line/host.ts`.
//!
//! # Divergences
//!
//! - `StatusLineHost` is not generic over the session; the session travels as
//!   `&dyn StatusLineSession` (a generic parameter cannot default to a trait
//!   object in Rust).
//! - `fetchUsageReports` and `lookupPullRequest` are synchronous: the crate has
//!   no async runtime and no `AbortSignal`.
//! - `Model`, `AgentMessage`, `Tool`, `UsageReport`, `UsageLimit` and
//!   `JobWidgetEntry` come from packages this crate does not have; they appear
//!   as [`serde_json::Value`].
//! - `StatusLineSession` / `FooterSession` are traits whose default methods
//!   mirror the reference's optional members.
//! - `segments::PromptCacheStatus` is the renderer-facing reduced form
//!   (`state` only); the full reference shape lives here.

use serde_json::Value;

use crate::status_line::schema::StatusLineSettings;

/// Account the session bills through, as far as the status line can see it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StatusAccountIdentity {
	/// Provider account id.
	pub account_id: Option<String>,
	/// Account email.
	pub email: Option<String>,
	/// Project id.
	pub project_id: Option<String>,
	/// Organization id.
	pub org_id: Option<String>,
}

/// Nested repository selected as the active workspace context.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActiveRepoContext {
	/// Repository working directory.
	pub cwd: String,
	/// Repository root.
	pub repo_root: String,
	/// Repository root relative to the project directory.
	pub relative_repo_root: String,
	/// How the repository was selected.
	pub source: String,
}

/// Why the next request rewrites the cached prompt prefix instead of reading it.
#[derive(Clone, Debug, PartialEq)]
pub enum PromptCacheColdReason {
	/// The entry outlived its lifetime.
	Expired {
		/// When it expired, in milliseconds since the epoch.
		at: i64,
	},
	/// The warm refresh found the entry gone.
	RefreshMissed,
	/// The model changed.
	Model {
		/// Previous model.
		from: String,
		/// Next model.
		to: String,
	},
	/// The thinking level changed.
	Thinking {
		/// Previous level.
		from: String,
		/// Next level.
		to: String,
	},
	/// The tool set changed.
	Tools,
	/// The system prompt changed.
	SystemPrompt,
	/// The retention tier changed.
	Retention,
	/// Compaction invalidated the entry.
	Compaction,
}

/// omp's cache warmer as it concerns the entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PromptCacheWarmingState {
	/// A refresh decision is armed.
	Scheduled,
	/// A refresh was sent and is in flight.
	Refreshing,
	/// Nothing is scheduled.
	Inactive,
}

/// omp's cache warmer as it concerns the entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PromptCacheWarming {
	/// Warmer state.
	pub state: PromptCacheWarmingState,
	/// When the armed refresh decides.
	pub next_warm_at: Option<i64>,
	/// When warming gives up: its 60-minute streaming or 30-minute idle window.
	pub stops_at: Option<i64>,
	/// Why nothing is scheduled.
	pub reason: Option<String>,
}

/// What the next request finds in the prompt cache.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PromptCacheState {
	/// The cached prefix is served.
	Warm,
	/// The next request rewrites the cached prefix.
	Cold,
}

/// Retention tier the entry was written under.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PromptCacheTier {
	/// Five minutes.
	Short,
	/// One hour.
	Long,
}

/// What the next request finds in the prompt cache: the `cache` segment's
/// marker and its details.
#[derive(Clone, Debug, PartialEq)]
pub struct PromptCacheStatus {
	/// Cache state.
	pub state: PromptCacheState,
	/// Why it is cold; absent while warm.
	pub reason: Option<PromptCacheColdReason>,
	/// Read off a reopened session's transcript, not a response this process saw.
	pub inferred: bool,
	/// Retention tier the entry was written under.
	pub tier: PromptCacheTier,
	/// Send time of the last request that read or wrote the entry.
	pub touched_at: i64,
	/// Lifetime after the last touch; `None` when the provider never expires it.
	pub ttl_ms: Option<i64>,
	/// Prompt tokens of the request that wrote or last read the entry.
	pub cached_tokens: u64,
	/// Estimated prompt tokens of the next request.
	pub next_tokens: u64,
	/// Warmer details; absent when the entry was inferred.
	pub warming: Option<PromptCacheWarming>,
}

/// Cumulative token and cost counters.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct UsageStatistics {
	/// Input tokens.
	pub input: u64,
	/// Output tokens.
	pub output: u64,
	/// Cache-read tokens.
	pub cache_read: u64,
	/// Cache-write tokens.
	pub cache_write: u64,
	/// Total tokens.
	pub total_tokens: u64,
	/// Orchestration input tokens.
	pub orchestration_input: u64,
	/// Orchestration output tokens.
	pub orchestration_output: u64,
	/// Orchestration cache-read tokens.
	pub orchestration_cache_read: u64,
	/// Premium requests.
	pub premium_requests: u64,
	/// Cost in dollars.
	pub cost: f64,
	/// Portion of `cost` carried by completed subagent task results.
	pub subagent_cost: Option<f64>,
}

/// Context-window occupancy.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ContextUsage {
	/// Tokens in the active window.
	pub tokens: u64,
	/// Window size.
	pub context_window: u64,
	/// Occupancy percent, or `None` when unknown.
	pub percent: Option<f64>,
}

/// Background speculative-compaction state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompactionSpeculation {
	/// No speculation.
	Idle,
	/// A speculative summary is being generated.
	Running,
	/// A speculation decision is armed.
	Armed,
}

/// Percent positions (0–100 of the context window) for the auto-compaction
/// boundaries.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CompactionBoundaries {
	/// Where auto-compaction fires.
	pub threshold_percent: f64,
	/// Where the background speculative summarizer starts, or `None` when no
	/// speculation will run.
	pub speculation_percent: Option<f64>,
}

/// One running background job.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AsyncJob {
	/// Job type.
	pub typ: String,
	/// Agent the job belongs to.
	pub agent_id: Option<String>,
}

/// Goal-mode state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GoalModeState {
	/// Goal status word.
	pub status: Option<String>,
	/// Tokens used by the goal.
	pub tokens_used: u64,
	/// Optional token budget.
	pub token_budget: Option<u64>,
}

/// One advisor's status in the roster.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AdvisorStatus {
	/// Advisor status word.
	pub status: String,
	/// Whether the advisor has yielded the turn.
	pub yielded: bool,
}

/// Advisor roster summary.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AdvisorStatusOverview {
	/// Whether at least one advisor is configured.
	pub configured: bool,
	/// Advisor roster.
	pub advisors: Vec<AdvisorStatus>,
}

/// The session facts the status-line renderers read.
///
/// The reference's `state` member (model, thinking level, messages) is split
/// into [`Self::model`], [`Self::thinking_level`] and [`Self::messages`] so the
/// crate's `SegmentContext` can be filled from the same source.
pub trait StatusLineSession {
	/// Active model; opaque because the catalog type is not in this crate.
	fn model(&self) -> Option<&Value> {
		None
	}
	/// Configured thinking level.
	fn thinking_level(&self) -> Option<&str> {
		None
	}
	/// Session transcript; opaque because `AgentMessage` is not in this crate.
	fn messages(&self) -> &[Value] {
		&[]
	}
	/// Session file path.
	fn session_file(&self) -> Option<&str> {
		None
	}
	/// Whether a turn is streaming.
	fn is_streaming(&self) -> bool {
		false
	}
	/// Whether auto thinking is enabled.
	fn is_auto_thinking(&self) -> bool {
		false
	}
	/// Revision of the context-usage reading.
	fn context_usage_revision(&self) -> Option<u64> {
		None
	}
	/// System prompt lines.
	fn system_prompt(&self) -> &[String] {
		&[]
	}
	/// Background speculative-compaction state.
	fn compaction_speculation(&self) -> Option<CompactionSpeculation> {
		None
	}
	/// Session title, if it has one.
	fn session_name(&self) -> Option<String> {
		None
	}
	/// Session id.
	fn session_id(&self) -> String {
		String::new()
	}
	/// Cumulative token and cost counters.
	fn usage_statistics(&self) -> UsageStatistics {
		UsageStatistics::default()
	}
	/// Whether `model` bills through a subscription.
	fn is_using_subscription(&self, model: &Value) -> bool;
	/// Context-window occupancy.
	fn context_usage(&self) -> Option<ContextUsage> {
		None
	}
	/// Auto-thinking level resolved from the last turn.
	fn auto_resolved_thinking_level(&self) -> Option<String> {
		None
	}
	/// Whether fast mode is active.
	fn is_fast_mode_active(&self) -> bool {
		false
	}
	/// Anthropic usage-limit label.
	fn anthropic_slow_mode_label(&self) -> Option<String> {
		None
	}
	/// Prewalk state; opaque.
	fn prewalk_state(&self) -> Option<&Value> {
		None
	}
	/// Running background jobs.
	fn async_job_snapshot(&self) -> Option<Vec<AsyncJob>> {
		None
	}
	/// Goal-mode state.
	fn goal_mode_state(&self) -> Option<GoalModeState> {
		None
	}
	/// Advisor roster summary.
	fn advisor_status_overview(&self) -> Option<AdvisorStatusOverview> {
		None
	}
	/// Advisor spend in dollars.
	fn advisor_cost(&self) -> Option<f64> {
		None
	}
	/// Whether the advisor bills through a subscription.
	fn advisor_using_subscription(&self) -> bool {
		false
	}
	/// What the next request finds in the prompt cache at `now_ms`.
	fn prompt_cache_status(&self, now_ms: i64, next_tokens: Option<u64>) -> Option<PromptCacheStatus> {
		let _ = (now_ms, next_tokens);
		None
	}
}

/// The session facts the footer reads.
pub trait FooterSession {
	/// Active model id and thinking support; opaque.
	fn model(&self) -> Option<&Value>;
	/// Configured thinking level.
	fn thinking_level(&self) -> Option<&str>;
	/// Whether auto thinking is enabled.
	fn is_auto_thinking(&self) -> bool;
	/// Auto-thinking level resolved from the last turn.
	fn auto_resolved_thinking_level(&self) -> Option<String>;
	/// Context-window occupancy.
	fn context_usage(&self) -> Option<ContextUsage>;
	/// Whether `model` bills through a subscription.
	fn is_using_subscription(&self, model: &Value) -> bool;
	/// Cumulative session rollup spanning every entry in the session.
	fn usage_statistics(&self) -> UsageStatistics;
}

/// Runtime services stay in the host; renderers only consume these capabilities.
pub trait StatusLineHost {
	/// Effective status-line settings.
	fn get_settings(&self) -> StatusLineSettings;
	/// Whether git facts are available.
	fn git_enabled(&self) -> bool;
	/// Whether the Codex reset fireworks effect is enabled.
	fn codex_reset_fireworks_enabled(&self) -> bool;
	/// Revision of the settings, bumped on every change.
	fn get_settings_revision(&self) -> u64;
	/// Opaque identity of the settings a session was started under.
	fn get_session_settings_identity(&self, session: &dyn StatusLineSession) -> Option<Value>;
	/// Revision of the settings a session was started under.
	fn get_session_settings_revision(&self, session: &dyn StatusLineSession) -> u64;
	/// Whether the goal status rides in the footer for this session.
	fn goal_status_in_footer(&self, session: &dyn StatusLineSession) -> bool;
	/// Account the session bills through.
	fn active_account(
		&self,
		session: &dyn StatusLineSession,
		provider: &str,
	) -> Option<StatusAccountIdentity>;
	/// Whether usage reports can be fetched for this session.
	fn can_fetch_usage_reports(&self, session: &dyn StatusLineSession) -> bool;
	/// Fetch usage reports; opaque because `UsageReport` is not in this crate.
	fn fetch_usage_reports(&self, session: &dyn StatusLineSession) -> Option<Value>;
	/// Resolve the nested repository selected as the workspace context.
	fn resolve_active_repo(&self, cwd: &str) -> Option<ActiveRepoContext>;
	/// Look up the pull request for `cwd`.
	fn lookup_pull_request(&self, cwd: &str) -> PullRequestOutcome;
	/// Tokens per second across `messages`, or `None` when it cannot be computed.
	fn calculate_tokens_per_second(&self, messages: &[Value], is_streaming: bool) -> Option<f64>;
	/// Whether a usage limit belongs to the session's active account.
	fn limit_matches_active_account(
		&self,
		report: &Value,
		limit: &Value,
		identity: &StatusAccountIdentity,
	) -> bool;
	/// Classify a usage report's reset expiry; opaque because `StatusResetExpiry`
	/// lives in the unported `types.ts`.
	fn classify_reset_expiry(&self, report: &Value, now_ms: i64) -> Option<Value> {
		let _ = (report, now_ms);
		None
	}
	/// One-line warning for saved resets about to expire; a returned warning
	/// counts as shown.
	fn reset_expiry_notice(
		&self,
		session: &dyn StatusLineSession,
		reports: &[Value],
		now_ms: i64,
	) -> Option<String> {
		let _ = (session, reports, now_ms);
		None
	}
	/// Auto-compaction boundary positions for the context gauge.
	fn compute_compaction_boundaries(
		&self,
		session: &dyn StatusLineSession,
		context_window: u64,
		model: Option<&Value>,
	) -> Option<CompactionBoundaries> {
		let _ = (session, context_window, model);
		None
	}
}

/// Outcome of a pull-request lookup.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PullRequestOutcome {
	/// Command stdout.
	pub stdout: String,
	/// Command exit code.
	pub exit_code: i32,
}

/// Footer runtime services.
pub trait FooterHost {
	/// Whether git facts are available.
	fn git_enabled(&self) -> bool;
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn usage_statistics_default_is_zeroed() {
		let usage = UsageStatistics::default();
		assert_eq!(usage.input, 0);
		assert_eq!(usage.cost, 0.0);
		assert_eq!(usage.subagent_cost, None);
	}

	#[test]
	fn prompt_cache_status_carries_cold_reason_and_warming() {
		let status = PromptCacheStatus {
			state: PromptCacheState::Cold,
			reason: Some(PromptCacheColdReason::Model {
				from: "a".to_string(),
				to: "b".to_string(),
			}),
			inferred: false,
			tier: PromptCacheTier::Long,
			touched_at: 7,
			ttl_ms: Some(60_000),
			cached_tokens: 10,
			next_tokens: 12,
			warming: Some(PromptCacheWarming {
				state: PromptCacheWarmingState::Scheduled,
				next_warm_at: Some(9),
				stops_at: None,
				reason: None,
			}),
		};
		assert_eq!(status.state, PromptCacheState::Cold);
		assert_eq!(status.tier, PromptCacheTier::Long);
		let warming = status.warming.expect("warming");
		assert_eq!(warming.state, PromptCacheWarmingState::Scheduled);
	}

	#[test]
	fn compaction_boundaries_keep_null_speculation() {
		let boundaries = CompactionBoundaries {
			threshold_percent: 80.0,
			speculation_percent: None,
		};
		assert_eq!(boundaries.speculation_percent, None);
	}

	#[test]
	fn active_repo_context_is_value_data() {
		let repo = ActiveRepoContext {
			cwd: "/w".to_string(),
			repo_root: "/w/repo".to_string(),
			relative_repo_root: "repo".to_string(),
			source: "single-direct-child-repo".to_string(),
		};
		assert_eq!(repo.source, "single-direct-child-repo");
		assert_eq!(repo.relative_repo_root, "repo");
	}

	#[test]
	fn optional_session_members_default_to_nothing() {
		struct Empty;
		impl StatusLineSession for Empty {
			fn is_using_subscription(&self, _model: &Value) -> bool {
				false
			}
		}
		let session = Empty;
		assert_eq!(session.model(), None);
		assert_eq!(session.messages(), &[] as &[Value]);
		assert_eq!(session.context_usage(), None);
		assert_eq!(session.prompt_cache_status(0, None), None);
		assert_eq!(session.usage_statistics(), UsageStatistics::default());
	}
}
