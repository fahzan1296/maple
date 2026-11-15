//! Segment renderers: one function per catalog segment.
//!
//! Ported from `l/src/status-line/segments.ts`. Every renderer is pure: it
//! reads [`SegmentContext`] and the active [`StatusTheme`] and returns a
//! [`RenderedSegment`]. The native/TSP `describe` side of the TypeScript
//! registry has no counterpart in this slice and is omitted.

use crate::status_line::schema::{SegmentId, SegmentOptions, TimeFormat};
use crate::status_line::{ActiveTheme, StatusTheme};
use crate::text_shape::{truncate_to_width, Ellipsis};

/// A segment's rendered text and whether it participates in the bar.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RenderedSegment {
    /// The segment text; may include ANSI color codes.
    pub content: String,
    /// Whether to render (for example, `git` is hidden outside a repository).
    pub visible: bool,
}

impl RenderedSegment {
    /// A hidden segment with no content.
    pub fn hidden() -> Self {
        RenderedSegment {
            content: String::new(),
            visible: false,
        }
    }

    /// A visible segment carrying `content`.
    pub fn shown(content: String) -> Self {
        RenderedSegment {
            content,
            visible: true,
        }
    }
}

/// Deterministic wall-clock reading used by previews and tests.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SegmentClock {
    /// Hour of day, 0–23.
    pub hour: u32,
    /// Minute of hour, 0–59.
    pub minute: u32,
    /// Second of minute, 0–59.
    pub second: u32,
    /// Epoch milliseconds; drives spinners and countdowns.
    pub epoch_ms: u64,
}

/// Per-token usage counters.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct UsageStats {
    /// Prompt input tokens.
    pub input: u64,
    /// Output tokens.
    pub output: u64,
    /// Cache-read tokens.
    pub cache_read: u64,
    /// Cache-write tokens.
    pub cache_write: u64,
    /// Cumulative total, cache read excluded.
    pub total_tokens: u64,
    /// Orchestration input tokens.
    pub orchestration_input: u64,
    /// Orchestration output tokens.
    pub orchestration_output: u64,
    /// Orchestration cache-read tokens.
    pub orchestration_cache_read: u64,
    /// Premium requests.
    pub premium_requests: f64,
    /// Session cost in dollars.
    pub cost: f64,
    /// Portion of `cost` carried by completed subagent task results.
    pub subagent_cost: Option<f64>,
    /// Tokens per second, or `None` when unknown.
    pub tokens_per_second: Option<f64>,
}

/// Git working-tree counts.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GitStatus {
    /// Staged file count.
    pub staged: u32,
    /// Unstaged file count.
    pub unstaged: u32,
    /// Untracked file count.
    pub untracked: u32,
}

impl GitStatus {
    /// True when any count is non-zero.
    pub fn is_dirty(&self) -> bool {
        self.staged > 0 || self.unstaged > 0 || self.untracked > 0
    }
}

/// A pull request attached to the current branch.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PullRequest {
    /// Pull-request number.
    pub number: u64,
    /// Pull-request URL.
    pub url: String,
}

/// Git facts available to the `git` and `pr` segments.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GitInfo {
    /// Current branch, or `None` outside a repository.
    pub branch: Option<String>,
    /// Working-tree counts, or `None` when unavailable.
    pub status: Option<GitStatus>,
    /// Pull request for the current branch.
    pub pr: Option<PullRequest>,
}

/// The repository the session is working in.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ActiveRepo {
    /// Repository working directory.
    pub cwd: String,
    /// Repository root relative to the project directory.
    pub relative_repo_root: String,
}

/// Project/worktree names for a linked git worktree.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WorktreeContext {
    /// Primary-checkout (project) name.
    pub project_name: String,
    /// Worktree directory name.
    pub worktree_name: String,
}

/// Model facts used by the `model` and `cost` segments.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ModelInfo {
    /// Catalog model name.
    pub name: Option<String>,
    /// Catalog model id.
    pub id: Option<String>,
    /// Whether the model supports thinking.
    pub thinking: bool,
    /// Configured thinking level.
    pub thinking_level: Option<String>,
}

/// One advisor's status in the roster.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AdvisorInfo {
    /// Advisor status word, e.g. `running`, `quota_exhausted`, `error`.
    pub status: String,
    /// Whether the advisor has yielded the turn.
    pub yielded: bool,
}

/// Advisor roster summary for the model segment badge.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AdvisorOverview {
    /// Whether at least one advisor is configured.
    pub configured: bool,
    /// Advisor roster.
    pub advisors: Vec<AdvisorInfo>,
}

/// Goal-mode state.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GoalState {
    /// Goal status word, e.g. `active`, `paused`, `complete`.
    pub status: Option<String>,
    /// Tokens used by the goal.
    pub tokens_used: u64,
    /// Optional token budget.
    pub token_budget: Option<u64>,
}

/// Session facts shared by several segments.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SessionInfo {
    /// Active model.
    pub model: ModelInfo,
    /// Whether auto thinking is enabled.
    pub is_auto_thinking: bool,
    /// Auto-thinking level resolved from the last turn.
    pub auto_resolved_thinking_level: Option<String>,
    /// Session id.
    pub session_id: Option<String>,
    /// Session title.
    pub session_name: Option<String>,
    /// Active named profile.
    pub profile: Option<String>,
    /// Whether fast mode is active.
    pub is_fast_mode_active: bool,
    /// Advisor roster.
    pub advisor: Option<AdvisorOverview>,
    /// Advisor spend in dollars.
    pub advisor_cost: f64,
    /// Whether the advisor is billed through a subscription.
    pub advisor_using_subscription: bool,
    /// Goal-mode state.
    pub goal: Option<GoalState>,
    /// Usage-limit stage label.
    pub slow_mode_label: Option<String>,
    /// Whether the model is billed through a subscription.
    pub using_subscription: bool,
}

/// A simple enabled/disabled mode.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EnabledMode {
    /// Whether the mode is enabled.
    pub enabled: bool,
}

/// Plan/goal mode state.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PausedMode {
    /// Whether the mode is enabled.
    pub enabled: bool,
    /// Whether the mode is paused.
    pub paused: bool,
}

/// Vim editing modes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum VimMode {
    /// Insert mode.
    Insert,
    /// Normal mode.
    #[default]
    Normal,
    /// Visual mode.
    Visual,
    /// Visual line mode.
    VisualLine,
    /// Replace mode.
    Replace,
}

impl VimMode {
    /// The mode's uppercase label.
    pub fn label(self) -> &'static str {
        match self {
            VimMode::Insert => "INSERT",
            VimMode::Normal => "NORMAL",
            VimMode::Visual => "VISUAL",
            VimMode::VisualLine => "V-LINE",
            VimMode::Replace => "REPLACE",
        }
    }

    /// The mode's symbol key.
    pub fn icon_key(self) -> &'static str {
        match self {
            VimMode::Insert => "icon.vimInsert",
            VimMode::Normal => "icon.vimNormal",
            VimMode::Visual => "icon.vimVisual",
            VimMode::VisualLine => "icon.vimVisualLine",
            VimMode::Replace => "icon.vimReplace",
        }
    }

    /// The mode's theme color token.
    pub fn color(self) -> &'static str {
        match self {
            VimMode::Insert => "success",
            VimMode::Normal => "accent",
            VimMode::Visual | VimMode::VisualLine => "warning",
            VimMode::Replace => "accent",
        }
    }
}

/// How the vim mode is presented.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum VimDisplay {
    /// Uppercase text label.
    #[default]
    Text,
    /// Symbol-theme glyph.
    Icon,
    /// Nothing.
    None,
}

/// Vim modal editing state.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct VimState {
    /// Active mode.
    pub mode: VimMode,
    /// Half-typed operator/count (`"2d"`), empty when nothing is pending.
    pub pending: String,
    /// Lines spanned by the active Visual selection.
    pub selected_lines: u32,
    /// How the mode renders.
    pub display: VimDisplay,
}

/// Collaboration role.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CollabRole {
    /// Session host.
    Host,
    /// Session guest.
    Guest,
}

/// Collaboration indicator.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CollabStatus {
    /// This participant's role.
    pub role: CollabRole,
    /// Participant count.
    pub participant_count: u32,
}

/// Live-stream indicator.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StreamStatus {
    /// Viewer count.
    pub viewers: u32,
}

/// Loop state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoopState {
    /// Waiting for the next prompt.
    Waiting,
    /// Running.
    Running,
    /// Paused.
    Paused,
}

impl LoopState {
    /// The state's label.
    pub fn label(self) -> &'static str {
        match self {
            LoopState::Waiting => "next prompt repeats",
            LoopState::Running => "running",
            LoopState::Paused => "paused",
        }
    }
}

/// Loop limit runtime.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoopLimit {
    /// Iteration budget.
    Iterations {
        /// Initial iteration count.
        initial: u64,
        /// Remaining iterations.
        remaining: u64,
    },
    /// Wall-clock deadline.
    Duration {
        /// Deadline in epoch milliseconds.
        deadline_ms: u64,
    },
}

/// A `/loop --while` / `--until` continue condition.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoopCondition {
    /// Shell command line.
    pub command: String,
    /// `--until` (continue while the command fails) when true.
    pub until: bool,
}

/// Loop mode state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoopMode {
    /// Loop state.
    pub state: LoopState,
    /// Optional limit.
    pub limit: Option<LoopLimit>,
    /// Optional continue condition.
    pub condition: Option<LoopCondition>,
}

/// Background speculative-compaction state.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Speculation {
    /// No background speculation.
    #[default]
    Idle,
    /// A speculation is running.
    Running,
    /// A speculation result is armed.
    Armed,
}

/// One quota window's usage.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct QuotaWindow {
    /// Used percentage.
    pub percent: f64,
    /// Minutes until reset, for short windows.
    pub reset_minutes: Option<u64>,
    /// Hours until reset, for long windows.
    pub reset_hours: Option<u64>,
}

/// How close a saved reset is to expiring.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExpiryTier {
    /// Within 7 days.
    Soon,
    /// Within 24 hours.
    Imminent,
}

/// A warning about expiring saved resets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResetExpiry {
    /// Warning tier.
    pub tier: ExpiryTier,
    /// Number of resets expiring.
    pub count: u64,
    /// Expiry of the warned-about reset, in epoch milliseconds.
    pub expires_at_ms: u64,
}

/// Saved-reset credits.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResetCredits {
    /// Banked reset count.
    pub banked_count: u64,
    /// Redeemable reset count.
    pub redeemable_count: u64,
    /// Hours until expiry.
    pub expiry_hours: Option<u64>,
    /// Whether the credits expired.
    pub expired: bool,
    /// Why the credits are unavailable.
    pub unavailable_reason: Option<String>,
    /// Optional expiring-reset warning.
    pub expiring: Option<ResetExpiry>,
}

/// Account quota windows and reset credits.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct UsageInfo {
    /// Account tier label.
    pub tier: Option<String>,
    /// Five-hour window.
    pub five_hour: Option<QuotaWindow>,
    /// Daily window.
    pub daily: Option<QuotaWindow>,
    /// Seven-day window.
    pub seven_day: Option<QuotaWindow>,
    /// Monthly window.
    pub monthly: Option<QuotaWindow>,
    /// Saved-reset credits.
    pub reset_credits: Option<ResetCredits>,
}

/// Prompt-cache warmth.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PromptCacheState {
    /// Cached prefix is warm.
    Warm,
    /// Prefix will be re-sent.
    Cold,
}

/// What the next request finds in the prompt cache.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PromptCacheStatus {
    /// Cache state.
    pub state: PromptCacheState,
}

/// Everything a segment renderer may read.
#[derive(Clone, Debug, Default)]
pub struct SegmentContext {
    /// Session facts.
    pub session: SessionInfo,
    /// Deterministic wall clock for previews and tests.
    pub now: Option<SegmentClock>,
    /// Deterministic host label for previews and tests.
    pub hostname: Option<String>,
    /// Focused subagent id while the view is proxied at its session.
    pub focused_agent_id: Option<String>,
    /// Effective `statusLine.sessionAccent`.
    pub session_accent: Option<bool>,
    /// Stand-in session title for previews.
    pub preview_title: Option<String>,
    /// The viewed session is pinned.
    pub is_pinned: bool,
    /// Project directory used when no repository is active.
    pub project_dir: String,
    /// Home directory used to abbreviate paths.
    pub home_dir: String,
    /// Active repository.
    pub active_repo: Option<ActiveRepo>,
    /// Viewport width.
    pub width: u16,
    /// Per-segment display options.
    pub options: SegmentOptions,
    /// Render the thinking level as a compact leading glyph.
    pub compact_thinking_level: bool,
    /// Key-sorted extension/hook status values.
    pub hook_statuses: Vec<String>,
    /// Plan mode.
    pub plan_mode: Option<PausedMode>,
    /// Prewalk mode.
    pub prewalk: Option<EnabledMode>,
    /// Loop mode.
    pub loop_mode: Option<LoopMode>,
    /// Whether the goal status rides in the footer.
    pub goal_status_in_footer: bool,
    /// Goal mode.
    pub goal_mode: Option<PausedMode>,
    /// Vibe mode.
    pub vibe_mode: Option<EnabledMode>,
    /// Vim modal state.
    pub vim: Option<VimState>,
    /// Collaboration indicator.
    pub collab: Option<CollabStatus>,
    /// Live-stream indicator.
    pub stream: Option<StreamStatus>,
    /// A `/record` capture is running.
    pub recording: bool,
    /// Token and cost counters.
    pub usage_stats: UsageStats,
    /// Context usage percent, or `None` when unknown.
    pub context_percent: Option<f64>,
    /// Estimated context tokens.
    pub context_tokens: u64,
    /// Context window size.
    pub context_window: u64,
    /// Auto-compaction is enabled.
    pub auto_compact_enabled: bool,
    /// Background speculative-compaction state.
    pub compaction_speculation: Speculation,
    /// Blink phase for the running-speculation pulse.
    pub speculation_blink_on: bool,
    /// Running subagent count.
    pub subagent_count: u32,
    /// Spend of every subagent under the main session.
    pub subagent_tree_cost: Option<f64>,
    /// Active processing time in milliseconds.
    pub active_ms: u64,
    /// Elapsed milliseconds of the running turn, or `None` when idle.
    pub turn_elapsed_ms: Option<u64>,
    /// Sampled foreground ANSI for the brand segment.
    pub brand_fg_ansi: Option<String>,
    /// Git facts.
    pub git: GitInfo,
    /// Linked-worktree names.
    pub worktree: Option<WorktreeContext>,
    /// Account quota windows.
    pub usage: Option<UsageInfo>,
    /// Prompt-cache state.
    pub prompt_cache: Option<PromptCacheStatus>,
    /// Whether terminal hyperlinks are enabled.
    pub hyperlinks: bool,
    /// Shared spinner period in milliseconds; zero keeps spinners static.
    pub spinner_interval_ms: u32,
}

// ═══════════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════════

/// Prefix `text` with `icon` and a space, unless the icon is empty.
pub fn with_icon(icon: &str, text: &str) -> String {
    if icon.is_empty() {
        text.to_string()
    } else {
        format!("{icon} {text}")
    }
}

/// Drop ANSI/OSC escape sequences and control characters, keeping text.
fn strip_display_escapes(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars();
    while let Some(ch) = chars.next() {
        if ch != '\x1b' {
            if ch.is_control() && !ch.is_whitespace() {
                continue;
            }
            out.push(ch);
            continue;
        }
        match chars.next() {
            Some('[') => {
                for next in chars.by_ref() {
                    if ('@'..='~').contains(&next) {
                        break;
                    }
                }
            }
            Some(']') => {
                let mut previous = '\0';
                for next in chars.by_ref() {
                    if next == '\x07' || (previous == '\x1b' && next == '\\') {
                        break;
                    }
                    previous = next;
                }
            }
            Some(_) | None => {}
        }
    }
    out
}

/// Compact single-line display text for statuses.
///
/// Mirrors `sanitizeStatusText`: strip escape/control sequences, expand
/// whitespace runs to one space, and trim the edges.
pub fn sanitize_status_text(text: &str) -> String {
    let stripped = strip_display_escapes(text);
    let mut out = String::with_capacity(stripped.len());
    let mut pending_space = false;
    for ch in stripped.chars() {
        if ch.is_whitespace() {
            pending_space = true;
        } else {
            if pending_space && !out.is_empty() {
                out.push(' ');
            }
            pending_space = false;
            out.push(ch);
        }
    }
    out
}

/// Format a number with K/M/B suffixes, matching `formatNumber`.
pub fn format_number(value: f64) -> String {
    if value < 1_000.0 {
        if value.fract() == 0.0 {
            return format!("{}", value as i64);
        }
        return value.to_string();
    }
    if value < 10_000.0 {
        return format!("{}K", trim1(value / 1_000.0));
    }
    if value < 1_000_000.0 {
        return format!("{}K", (value / 1_000.0).round() as i64);
    }
    if value < 10_000_000.0 {
        return format!("{}M", trim1(value / 1_000_000.0));
    }
    if value < 1_000_000_000.0 {
        return format!("{}M", (value / 1_000_000.0).round() as i64);
    }
    if value < 10_000_000_000.0 {
        return format!("{}B", trim1(value / 1_000_000_000.0));
    }
    format!("{}B", (value / 1_000_000_000.0).round() as i64)
}

fn trim1(value: f64) -> String {
    let text = format!("{value:.1}");
    text.strip_suffix(".0").unwrap_or(&text).to_string()
}

/// Format a duration in milliseconds, matching `formatDuration`.
pub fn format_duration(ms: u64) -> String {
    if ms == 0 {
        return "0ms".to_string();
    }
    if ms < 1_000 {
        return format!("{ms}ms");
    }
    if ms < 60_000 {
        let tenths = (ms as f64 / 100.0).round() as u64;
        return if tenths < 600 {
            format!("{:.1}s", tenths as f64 / 10.0)
        } else {
            "1m".to_string()
        };
    }
    if ms < 3_600_000 {
        let mins = ms / 60_000;
        let secs = (ms % 60_000) / 1_000;
        return if secs > 0 {
            format!("{mins}m{secs}s")
        } else {
            format!("{mins}m")
        };
    }
    if ms < 86_400_000 {
        let hours = ms / 3_600_000;
        let mins = (ms % 3_600_000) / 60_000;
        return if mins > 0 {
            format!("{hours}h{mins}m")
        } else {
            format!("{hours}h")
        };
    }
    let days = ms / 86_400_000;
    let hours = (ms % 86_400_000) / 3_600_000;
    if hours > 0 {
        format!("{days}d{hours}h")
    } else {
        format!("{days}d")
    }
}

/// Replace a leading home directory with `~`.
pub fn shorten_path(path: &str, home: &str) -> String {
    if home.is_empty() {
        return path.to_string();
    }
    if path == home {
        return "~".to_string();
    }
    if let Some(rest) = path.strip_prefix(home) {
        if let Some(stripped) = rest.strip_prefix('/') {
            return format!("~/{stripped}");
        }
    }
    path.to_string()
}

/// Wrap `text` in an OSC-8 file hyperlink when hyperlinks are enabled.
pub fn file_hyperlink(path: &str, text: &str, enabled: bool) -> String {
    if !enabled {
        return text.to_string();
    }
    format!("\x1b]8;;file://{path}\x07{text}\x1b]8;;\x07")
}

/// Left-truncate a path/label to `max_len` characters, prefixing an ellipsis.
pub fn clamp_path_length(value: &str, max_len: usize) -> String {
    let count = value.chars().count();
    if count <= max_len {
        return value.to_string();
    }
    let keep = max_len.saturating_sub(1);
    let tail: String = value.chars().skip(count - keep).collect();
    format!("…{tail}")
}

/// Format a metric with an optional leading icon.
pub fn format_metric(leading: Option<&str>, value: &str) -> String {
    match leading {
        Some(leading) if !leading.is_empty() => format!("{leading} {value}"),
        _ => value.to_string(),
    }
}

/// Hash-derived accent ANSI for the session title, when enabled.
fn session_accent_ansi(theme: &dyn StatusTheme, ctx: &SegmentContext) -> Option<String> {
    if ctx.session_accent == Some(false) {
        return None;
    }
    let name = ctx
        .session
        .session_name
        .clone()
        .or_else(|| ctx.preview_title.clone())?;
    theme.session_accent_ansi(&name)
}

/// `theme.fg` for accent-role text: the session accent when enabled.
fn accent_fg(theme: &dyn StatusTheme, ctx: &SegmentContext, color: &str, text: &str) -> String {
    let ansi = session_accent_ansi(theme, ctx).unwrap_or_else(|| theme.fg_ansi(color));
    format!("{ansi}{text}\x1b[39m")
}

/// Frame index on the shared spinner clock.
fn shared_spinner_frame(frame_count: usize, now_ms: u64, interval_ms: u32) -> usize {
    if frame_count == 0 || interval_ms == 0 {
        return 0;
    }
    (now_ms / interval_ms as u64) as usize % frame_count
}

/// Brand glyph tagged with the active named profile.
fn brand_mark(glyph: &str, profile: &Option<String>) -> String {
    match profile {
        None => glyph.to_string(),
        Some(profile) if !profile.is_empty() && !glyph.is_empty() => {
            format!("{glyph}-{profile}")
        }
        Some(profile) => profile.clone(),
    }
}

/// Turn timer in omp's brand format.
fn brand_timer(elapsed_ms: u64) -> String {
    let seconds = elapsed_ms / 1_000;
    if seconds < 60 {
        format!("{seconds}s")
    } else if seconds < 3_600 {
        format!("{}m", seconds / 60)
    } else {
        format!("{}h", (seconds / 3_600).min(99))
    }
}

fn brand_spinner_frame(theme: &dyn StatusTheme, ctx: &SegmentContext) -> String {
    let frames = theme.spinner_frames("activity");
    if frames.is_empty() {
        return String::new();
    }
    let now = ctx.now.map(|clock| clock.epoch_ms).unwrap_or(0);
    let index = shared_spinner_frame(frames.len(), now, ctx.spinner_interval_ms);
    frames.get(index).cloned().unwrap_or_default()
}

/// Display name of the active model (`Claude ` prefix dropped).
fn model_display_name(ctx: &SegmentContext) -> String {
    let model = &ctx.session.model;
    let name = model
        .name
        .clone()
        .filter(|name| !name.is_empty())
        .or_else(|| model.id.clone().filter(|id| !id.is_empty()))
        .unwrap_or_else(|| "no-model".to_string());
    name.strip_prefix("Claude ")
        .map(str::to_string)
        .unwrap_or(name)
}

/// The thinking-effort word, or `None` when the model has no thinking.
fn thinking_level_word(ctx: &SegmentContext) -> Option<String> {
    let model = &ctx.session.model;
    if !model.thinking {
        return None;
    }
    if ctx.session.is_auto_thinking {
        return Some(
            ctx.session
                .auto_resolved_thinking_level
                .clone()
                .unwrap_or_else(|| "auto".to_string()),
        );
    }
    Some(
        model
            .thinking_level
            .clone()
            .unwrap_or_else(|| "off".to_string()),
    )
}

/// Thinking-level display (`"◒ high"`, `"⟳ auto"`, …), or empty when hidden.
fn model_thinking_display(theme: &dyn StatusTheme, ctx: &SegmentContext) -> String {
    let model = &ctx.session.model;
    let opts = ctx.options.model.clone().unwrap_or_default();
    if !opts.show_thinking_level() || !model.thinking {
        return String::new();
    }
    if ctx.session.is_auto_thinking {
        return match &ctx.session.auto_resolved_thinking_level {
            Some(level) => {
                let key = format!("thinking.{level}");
                let symbol = theme.symbol(&key);
                if symbol.is_empty() {
                    level.clone()
                } else {
                    symbol
                }
            }
            None => format!("{} auto", theme.symbol("thinking.autoPending")),
        };
    }
    let level = model
        .thinking_level
        .clone()
        .unwrap_or_else(|| "off".to_string());
    if level == "off" {
        format!("{} off", theme.symbol("status.disabled"))
    } else {
        let key = format!("thinking.{level}");
        let symbol = theme.symbol(&key);
        if symbol.is_empty() {
            level
        } else {
            symbol
        }
    }
}

/// Leading glyph of a thinking-level display string.
fn leading_glyph(display: &str) -> &str {
    match display.find(' ') {
        Some(index) => &display[..index],
        None => display,
    }
}

/// Advisor symbol colored by the worst status in the roster.
fn model_advisor_badge(
    theme: &dyn StatusTheme,
    ctx: &SegmentContext,
) -> Option<(String, &'static str)> {
    let overview = ctx.session.advisor.as_ref()?;
    if !overview.configured || overview.advisors.is_empty() {
        return None;
    }
    let statuses: Vec<&str> = overview.advisors.iter().map(|a| a.status.as_str()).collect();
    let color = if statuses.contains(&"error") {
        "error"
    } else if statuses.contains(&"quota_exhausted") {
        "warning"
    } else if statuses.contains(&"running") {
        "success"
    } else {
        "dim"
    };
    let all_yielded = overview.advisors.iter().all(|a| a.yielded);
    let icon = if all_yielded {
        let closed = theme.symbol("icon.advisorClosed");
        if closed.is_empty() {
            theme.symbol("icon.advisor")
        } else {
            closed
        }
    } else {
        theme.symbol("icon.advisor")
    };
    if icon.is_empty() {
        None
    } else {
        Some((icon, color))
    }
}

fn format_goal_budget(current: u64, budget: Option<u64>) -> String {
    let used = format_number(current as f64);
    match budget {
        Some(budget) => format!("{}/{}", used, format_number(budget as f64)),
        None => used,
    }
}

fn format_loop_limit(limit: &LoopLimit, now_ms: u64) -> Option<String> {
    match limit {
        LoopLimit::Iterations {
            initial,
            remaining,
        } => Some(format!("{remaining}/{initial}")),
        LoopLimit::Duration { deadline_ms } => {
            let remaining = (*deadline_ms as i64 - now_ms as i64).max(0);
            let total_seconds = (remaining + 999) / 1_000;
            let hours = total_seconds / 3_600;
            let minutes = (total_seconds % 3_600) / 60;
            let seconds = total_seconds % 60;
            if hours > 0 {
                Some(if minutes > 0 {
                    format!("{hours}h{minutes}m left")
                } else {
                    format!("{hours}h left")
                })
            } else if minutes > 0 {
                Some(if seconds > 0 {
                    format!("{minutes}m{seconds}s left")
                } else {
                    format!("{minutes}m left")
                })
            } else {
                Some(format!("{seconds}s left"))
            }
        }
    }
}

fn summarize_loop_condition(condition: &LoopCondition, max_width: usize) -> String {
    let label = if condition.until { "until" } else { "while" };
    let command = sanitize_status_text(&condition.command);
    let budget = max_width.saturating_sub(label.len() + 2).max(1);
    format!(
        "{}: {}",
        label,
        truncate_to_width(&command, budget, Ellipsis::Unicode)
    )
}

/// Context-window occupancy level.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContextUsageLevel {
    /// Comfortable.
    Normal,
    /// Warning threshold reached.
    Warning,
    /// Purple threshold reached.
    Purple,
    /// Error threshold reached.
    Error,
}

fn reaches_threshold(percent: f64, window: u64, percent_threshold: f64, token_threshold: f64) -> bool {
    if !percent.is_finite() || percent <= 0.0 {
        return false;
    }
    if window == 0 {
        return percent >= percent_threshold;
    }
    let token_percent = (token_threshold / window as f64) * 100.0;
    percent >= percent_threshold.min(token_percent)
}

/// Context usage level from percent and window.
pub fn context_usage_level(percent: f64, window: u64) -> ContextUsageLevel {
    if reaches_threshold(percent, window, 90.0, 500_000.0) {
        return ContextUsageLevel::Error;
    }
    if reaches_threshold(percent, window, 70.0, 270_000.0) {
        return ContextUsageLevel::Purple;
    }
    if reaches_threshold(percent, window, 50.0, 150_000.0) {
        return ContextUsageLevel::Warning;
    }
    ContextUsageLevel::Normal
}

/// Theme color token for a context-usage level.
pub fn context_usage_color(level: ContextUsageLevel) -> &'static str {
    match level {
        ContextUsageLevel::Error => "error",
        ContextUsageLevel::Purple => "thinkingHigh",
        ContextUsageLevel::Warning => "warning",
        ContextUsageLevel::Normal => "statusLineContext",
    }
}

/// Format context usage as `<percent>%/<window>` when the window is known.
pub fn format_context_usage(percent: Option<f64>, window: u64, used_tokens: u64) -> String {
    if window == 0 {
        return format!("{}/?", format_number(used_tokens as f64));
    }
    let pct = match percent {
        Some(value) => format!("{value:.1}%"),
        None => "?".to_string(),
    };
    format!("{pct}/{}", format_number(window as f64))
}

fn pick_usage_color(percent: f64) -> &'static str {
    if percent >= 80.0 {
        "error"
    } else if percent >= 50.0 {
        "warning"
    } else {
        "muted"
    }
}

fn format_usage_reset(value: u64, unit: char) -> String {
    if unit == 'm' {
        if value < 60 {
            return format!("{value}m");
        }
        let hours = value / 60;
        let mins = value % 60;
        return if mins > 0 {
            format!("{hours}h {mins}m")
        } else {
            format!("{hours}h")
        };
    }
    if value < 24 {
        return format!("{value}h");
    }
    let days = value / 24;
    let hours = value % 24;
    if hours > 0 {
        format!("{days}d {hours}h")
    } else {
        format!("{days}d")
    }
}

fn reset_credits_color(resets: &ResetCredits) -> &'static str {
    if let Some(expiring) = &resets.expiring {
        return match expiring.tier {
            ExpiryTier::Imminent => "error",
            ExpiryTier::Soon => "warning",
        };
    }
    if resets.redeemable_count > 0 {
        "success"
    } else {
        "warning"
    }
}

fn normalize_premium_requests(value: f64) -> f64 {
    ((value + f64::EPSILON) * 100.0).round() / 100.0
}

fn format_spend(
    amount: f64,
    using_subscription: bool,
    fraction_digits: usize,
    theme: &dyn StatusTheme,
    show_unit: bool,
) -> String {
    let formatted = format!("{amount:.fraction_digits$}");
    if !show_unit {
        return formatted;
    }
    if !using_subscription {
        return format!("${formatted}");
    }
    if theme.symbol_preset() == "nerd" {
        let icon = theme.symbol("icon.subscription");
        if !icon.is_empty() {
            return format!("{icon} {formatted}");
        }
    }
    format!("S{formatted}")
}

fn format_advisor_spend(
    amount: f64,
    using_subscription: bool,
    fraction_digits: usize,
    theme: &dyn StatusTheme,
    show_unit: bool,
) -> String {
    let spend = format_spend(amount, using_subscription, fraction_digits, theme, show_unit);
    let icon = theme.symbol("icon.advisor");
    if !icon.is_empty() && icon != "(adv)" {
        format!("{icon} {spend}")
    } else {
        format!("{spend} (adv)")
    }
}

/// Billing summary for the `cost` segment, or `None` when there is nothing to bill.
pub fn billing_summary(ctx: &SegmentContext, theme: &dyn StatusTheme) -> Option<String> {
    let task_result_cost = ctx.usage_stats.subagent_cost.unwrap_or(0.0);
    let own_cost = (ctx.usage_stats.cost - task_result_cost).max(0.0);
    let subagent_cost = ctx.subagent_tree_cost.unwrap_or(0.0).max(task_result_cost);
    let advisor_cost = ctx.session.advisor_cost;
    let premium_requests = normalize_premium_requests(ctx.usage_stats.premium_requests);
    let using_subscription = ctx.session.using_subscription;
    if own_cost == 0.0
        && subagent_cost == 0.0
        && advisor_cost == 0.0
        && !using_subscription
        && premium_requests == 0.0
    {
        return None;
    }

    let mut parts: Vec<String> = Vec::new();
    let mut shown_subscription = using_subscription;
    if own_cost != 0.0 {
        parts.push(format_spend(
            own_cost,
            using_subscription,
            2,
            theme,
            true,
        ));
    } else if using_subscription {
        let icon = if theme.symbol_preset() == "nerd" {
            theme.symbol("icon.subscription")
        } else {
            String::new()
        };
        parts.push(if icon.is_empty() {
            "(sub)".to_string()
        } else {
            icon
        });
        shown_subscription = true;
    }
    if subagent_cost != 0.0 {
        parts.push(format!("(+{:.2})", subagent_cost));
    }
    if premium_requests != 0.0 {
        parts.push(format!("★ {}", format_number(premium_requests)));
    }
    if advisor_cost != 0.0 {
        let prefix = if parts.is_empty() { "" } else { "+ " };
        parts.push(format!(
            "{prefix}{}",
            format_advisor_spend(
                advisor_cost,
                ctx.session.advisor_using_subscription,
                2,
                theme,
                shown_subscription && !ctx.session.advisor_using_subscription,
            )
        ));
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(" "))
    }
}

/// Relative path of `path` under `root`, when it is inside.
fn relative_within(root: &str, path: &str) -> Option<String> {
    if path == root {
        return Some(String::new());
    }
    let rest = path.strip_prefix(root)?;
    let rest = rest.strip_prefix('/')?;
    Some(rest.to_string())
}

/// Classify a project directory as scratch or a display-root-relative path.
fn project_dir_display(ctx: &SegmentContext, project_dir: &str) -> (bool, String) {
    const SCRATCH_ROOTS: &[&str] = &["/tmp", "/var/tmp"];
    for root in SCRATCH_ROOTS {
        if let Some(relative) = relative_within(root, project_dir) {
            return (true, if relative.is_empty() { project_dir.to_string() } else { relative });
        }
    }
    let display_roots = [
        format!("{}/Projects", ctx.home_dir),
        format!("{}/repos", ctx.home_dir),
        "/work".to_string(),
    ];
    for root in &display_roots {
        if let Some(relative) = relative_within(root, project_dir) {
            return (false, relative);
        }
    }
    (false, project_dir.to_string())
}

// ═══════════════════════════════════════════════════════════════════════════
// Segment Implementations
// ═══════════════════════════════════════════════════════════════════════════

fn pi_segment(theme: &dyn StatusTheme, ctx: &SegmentContext) -> RenderedSegment {
    if let Some(agent) = &ctx.focused_agent_id {
        let ghost = theme.symbol("icon.ghost");
        let icon = if ghost.is_empty() {
            String::new()
        } else {
            format!("{ghost} ")
        };
        return RenderedSegment::shown(theme.fg("warning", &format!("{icon}{agent}")));
    }
    let fg = ctx
        .brand_fg_ansi
        .clone()
        .unwrap_or_else(|| theme.fg_ansi("dim"));
    let opts = ctx.options.pi.clone().unwrap_or_default();
    let profile = if opts.show_profile() {
        ctx.session.profile.clone()
    } else {
        None
    };
    let content = match ctx.turn_elapsed_ms {
        Some(elapsed) => format!(
            "{} {}",
            brand_mark(&brand_spinner_frame(theme, ctx), &profile),
            brand_timer(elapsed)
        ),
        None => brand_mark(&theme.symbol("icon.omp"), &profile),
    };
    RenderedSegment::shown(format!("{fg}{content}\x1b[39m"))
}

fn status_segment(theme: &dyn StatusTheme, ctx: &SegmentContext) -> RenderedSegment {
    let mut text = String::new();
    for status in &ctx.hook_statuses {
        let sanitized = sanitize_status_text(status);
        if sanitized.is_empty() {
            continue;
        }
        if text.is_empty() {
            text = sanitized;
        } else {
            text.push_str(&theme.sep("dot"));
            text.push_str(&sanitized);
        }
    }
    if text.is_empty() {
        RenderedSegment::hidden()
    } else {
        RenderedSegment::shown(accent_fg(theme, ctx, "accent", &text))
    }
}

fn model_segment(theme: &dyn StatusTheme, ctx: &SegmentContext) -> RenderedSegment {
    let model_name = model_display_name(ctx);
    let thinking_display = model_thinking_display(theme, ctx);
    let compact = ctx.compact_thinking_level && !thinking_display.is_empty();
    let model_icon = if compact {
        leading_glyph(&thinking_display).to_string()
    } else {
        theme.symbol("icon.model")
    };

    let mut tail = String::new();
    if ctx.session.is_fast_mode_active {
        let fast = theme.symbol("icon.fast");
        if !fast.is_empty() {
            tail.push_str(&format!(" {fast}"));
        }
    }
    if !compact && !thinking_display.is_empty() {
        tail.push_str(&theme.sep("dot"));
        tail.push_str(&thinking_display);
    }

    let mut content = accent_fg(
        theme,
        ctx,
        "statusLineModel",
        &with_icon(&model_icon, &model_name),
    );
    if let Some((icon, color)) = model_advisor_badge(theme, ctx) {
        content.push_str(&theme.fg(color, &format!(" {icon}")));
    }
    if !tail.is_empty() {
        content.push_str(&accent_fg(theme, ctx, "statusLineModel", &tail));
    }
    if let Some(label) = &ctx.session.slow_mode_label {
        content.push_str(&theme.fg(
            "warning",
            &format!("{}{}", theme.sep("dot"), label),
        ));
    }
    RenderedSegment::shown(content)
}

fn render_goal_mode(theme: &dyn StatusTheme, ctx: &SegmentContext, mode: PausedMode) -> RenderedSegment {
    let goal = ctx.session.goal.as_ref();
    let status = goal
        .and_then(|goal| goal.status.clone())
        .unwrap_or_else(|| if mode.paused { "paused".to_string() } else { "active".to_string() });

    let color: &str = match status.as_str() {
        "paused" => "warning",
        "complete" => "success",
        "budget-limited" => "warning",
        "dropped" => "dim",
        _ => "accent",
    };
    let icon = match status.as_str() {
        "paused" => {
            let pause = theme.symbol("icon.pause");
            if pause.is_empty() {
                theme.symbol("status.pending")
            } else {
                pause
            }
        }
        "complete" => theme.symbol("status.success"),
        "budget-limited" => theme.symbol("status.warning"),
        "dropped" => theme.symbol("status.aborted"),
        _ => theme.symbol("icon.goal"),
    };

    let mut parts = vec![with_icon(&icon, "Goal")];
    if ctx.goal_status_in_footer {
        if let Some(goal) = goal {
            parts.push(format_goal_budget(goal.tokens_used, goal.token_budget));
        }
    }
    let content = if color == "accent" {
        accent_fg(theme, ctx, color, &parts.join(" "))
    } else {
        theme.fg(color, &parts.join(" "))
    };
    RenderedSegment::shown(content)
}

fn mode_segment(theme: &dyn StatusTheme, ctx: &SegmentContext) -> RenderedSegment {
    let pause_suffix = {
        let pause = theme.symbol("icon.pause");
        if pause.is_empty() {
            " (paused)".to_string()
        } else {
            format!(" {pause}")
        }
    };

    if let Some(plan) = ctx.plan_mode {
        if plan.enabled || plan.paused {
            let label = if plan.paused {
                format!("Plan{pause_suffix}")
            } else {
                "Plan".to_string()
            };
            let content = with_icon(&theme.symbol("icon.plan"), &label);
            return RenderedSegment::shown(if plan.paused {
                theme.fg("warning", &content)
            } else {
                accent_fg(theme, ctx, "accent", &content)
            });
        }
    }

    if let Some(prewalk) = ctx.prewalk {
        if prewalk.enabled {
            let content = with_icon(&theme.symbol("icon.prewalk"), "Prewalk");
            return RenderedSegment::shown(accent_fg(theme, ctx, "accent", &content));
        }
    }

    if let Some(goal) = ctx.goal_mode {
        if goal.enabled || goal.paused {
            return render_goal_mode(theme, ctx, goal);
        }
    }

    if let Some(vibe) = ctx.vibe_mode {
        if vibe.enabled {
            let content = with_icon(&theme.symbol("icon.agents"), "Vibe");
            return RenderedSegment::shown(accent_fg(theme, ctx, "accent", &content));
        }
    }

    if let Some(loop_mode) = &ctx.loop_mode {
        let paused = loop_mode.state == LoopState::Paused;
        let loop_icon = theme.symbol("icon.loop");
        let icon = if paused {
            let pause = theme.symbol("icon.pause");
            if pause.is_empty() {
                loop_icon
            } else {
                pause
            }
        } else {
            loop_icon
        };
        let color = if paused { "warning" } else { "customMessageLabel" };
        let label = format!(
            "Loop{} {}",
            if loop_mode.state == LoopState::Waiting {
                ":"
            } else {
                ""
            },
            loop_mode.state.label()
        );
        let mut parts = vec![with_icon(&icon, &label)];
        let now = ctx.now.map(|clock| clock.epoch_ms).unwrap_or(0);
        if let Some(limit) = &loop_mode.limit {
            if let Some(text) = format_loop_limit(limit, now) {
                parts.push(text);
            }
        }
        if let Some(condition) = &loop_mode.condition {
            parts.push(summarize_loop_condition(condition, 40));
        }
        return RenderedSegment::shown(theme.fg(color, &parts.join(" ")));
    }

    RenderedSegment::hidden()
}

fn path_segment(theme: &dyn StatusTheme, ctx: &SegmentContext) -> RenderedSegment {
    let opts = ctx.options.path.clone().unwrap_or_default();
    let strip_prefix = opts.strip_work_prefix();

    if strip_prefix {
        if let Some(worktree) = &ctx.worktree {
            let label = if ctx.git.branch.as_deref() == Some(worktree.worktree_name.as_str()) {
                worktree.project_name.clone()
            } else {
                format!("{}/{}", worktree.project_name, worktree.worktree_name)
            };
            let text = file_hyperlink(
                &ctx.project_dir,
                &clamp_path_length(&label, opts.max_length()),
                ctx.hyperlinks,
            );
            let content = with_icon(&theme.symbol("icon.worktree"), &text);
            return RenderedSegment::shown(theme.fg("statusLinePath", &content));
        }
    }
    let project_dir = ctx
        .active_repo
        .as_ref()
        .map(|repo| repo.cwd.clone())
        .unwrap_or_else(|| ctx.project_dir.clone());
    let (scratch, display_path) = project_dir_display(ctx, &project_dir);
    let mut pwd = if strip_prefix {
        display_path
    } else {
        project_dir.clone()
    };
    let repo_suffix = ctx
        .active_repo
        .as_ref()
        .map(|repo| format!(" ↳ {}", repo.relative_repo_root))
        .unwrap_or_default();
    if opts.abbreviate() {
        pwd = shorten_path(&pwd, &ctx.home_dir);
    }
    pwd = clamp_path_length(&pwd, opts.max_length());

    let icon_key = if scratch && strip_prefix {
        "icon.scratchFolder"
    } else {
        "icon.folder"
    };
    let text = format!(
        "{}{}",
        file_hyperlink(&project_dir, &pwd, ctx.hyperlinks),
        repo_suffix
    );
    let content = with_icon(&theme.symbol(icon_key), &text);
    RenderedSegment::shown(theme.fg("statusLinePath", &content))
}

fn git_segment(theme: &dyn StatusTheme, ctx: &SegmentContext) -> RenderedSegment {
    let branch = ctx.git.branch.clone();
    let status = ctx.git.status.clone();
    if branch.is_none() && status.is_none() {
        return RenderedSegment::hidden();
    }

    let opts = ctx.options.git.clone().unwrap_or_default();
    let is_dirty = status.as_ref().map(GitStatus::is_dirty).unwrap_or(false);
    let show_branch = opts.show_branch();

    let mut content = String::new();
    if show_branch {
        if let Some(branch) = &branch {
            content = with_icon(&theme.symbol("icon.branch"), branch);
        }
    }

    if let Some(status) = &status {
        let mut indicators: Vec<String> = Vec::new();
        if opts.show_unstaged() && status.unstaged > 0 {
            indicators.push(theme.fg(
                "statusLineDirty",
                &format!("*{}", status.unstaged),
            ));
        }
        if opts.show_staged() && status.staged > 0 {
            indicators.push(theme.fg(
                "statusLineStaged",
                &format!("+{}", status.staged),
            ));
        }
        if opts.show_untracked() && status.untracked > 0 {
            indicators.push(theme.fg(
                "statusLineUntracked",
                &format!("?{}", status.untracked),
            ));
        }
        if !indicators.is_empty() {
            let indicator_text = indicators.join(" ");
            if content.is_empty() && !show_branch {
                content = with_icon(&theme.symbol("icon.git"), &indicator_text);
            } else if content.is_empty() {
                content = indicator_text;
            } else {
                content.push(' ');
                content.push_str(&indicator_text);
            }
        }
    }

    if content.is_empty() {
        return RenderedSegment::hidden();
    }

    let color = if is_dirty {
        "statusLineGitDirty"
    } else {
        "statusLineGitClean"
    };
    RenderedSegment::shown(theme.fg(color, &content))
}

fn pr_segment(theme: &dyn StatusTheme, ctx: &SegmentContext) -> RenderedSegment {
    let pr = match &ctx.git.pr {
        Some(pr) => pr,
        None => return RenderedSegment::hidden(),
    };
    let label = with_icon(&theme.symbol("icon.pr"), &format!("#{}", pr.number));
    let content = if ctx.hyperlinks {
        format!("\x1b]8;;{}\x07{label}\x1b]8;;\x07", pr.url)
    } else {
        label
    };
    RenderedSegment::shown(accent_fg(theme, ctx, "accent", &content))
}

fn subagents_segment(theme: &dyn StatusTheme, ctx: &SegmentContext) -> RenderedSegment {
    if ctx.subagent_count == 0 {
        return RenderedSegment::hidden();
    }
    let content = with_icon(
        &theme.symbol("icon.agents"),
        &format!("{}", ctx.subagent_count),
    );
    RenderedSegment::shown(theme.fg("statusLineSubagents", &content))
}

fn single_stat_segment(
    field: &str,
    icon_key: &str,
    color: &'static str,
    theme: &dyn StatusTheme,
    ctx: &SegmentContext,
) -> RenderedSegment {
    let value = match field {
        "input" => ctx.usage_stats.input,
        "output" => ctx.usage_stats.output,
        "cache_read" => ctx.usage_stats.cache_read,
        "cache_write" => ctx.usage_stats.cache_write,
        _ => 0,
    };
    if value == 0 {
        return RenderedSegment::hidden();
    }
    let content = format_metric(
        Some(&theme.symbol(icon_key)),
        &format_number(value as f64),
    );
    RenderedSegment::shown(theme.fg(color, &content))
}

fn token_total_segment(theme: &dyn StatusTheme, ctx: &SegmentContext) -> RenderedSegment {
    let stats = &ctx.usage_stats;
    let total = stats
        .input
        .saturating_add(stats.output)
        .saturating_add(stats.cache_write)
        .saturating_add(stats.orchestration_input)
        .saturating_add(stats.orchestration_output);
    if total == 0 {
        return RenderedSegment::hidden();
    }
    let content = format_metric(
        Some(&theme.symbol("icon.tokens")),
        &format_number(total as f64),
    );
    RenderedSegment::shown(theme.fg("statusLineSpend", &content))
}

fn token_rate_segment(theme: &dyn StatusTheme, ctx: &SegmentContext) -> RenderedSegment {
    let rate = match ctx.usage_stats.tokens_per_second {
        Some(rate) if rate != 0.0 && rate.is_finite() => rate,
        _ => return RenderedSegment::hidden(),
    };
    let content = format_metric(
        Some(&theme.symbol("icon.throughput")),
        &format!("{rate:.1} tok/s"),
    );
    RenderedSegment::shown(theme.fg("statusLineOutput", &content))
}

fn cost_segment(theme: &dyn StatusTheme, ctx: &SegmentContext) -> RenderedSegment {
    match billing_summary(ctx, theme) {
        Some(billing) => RenderedSegment::shown(theme.fg("statusLineCost", &billing)),
        None => RenderedSegment::hidden(),
    }
}

fn context_pct_segment(theme: &dyn StatusTheme, ctx: &SegmentContext) -> RenderedSegment {
    let percent = ctx.context_percent;
    let window = ctx.context_window;
    let color = context_usage_color(context_usage_level(percent.unwrap_or(0.0), window));

    let mut auto_icon = String::new();
    let auto = theme.symbol("icon.auto");
    if ctx.auto_compact_enabled && !auto.is_empty() {
        let accent_icon = accent_fg(theme, ctx, "accent", &auto);
        let styled = match ctx.compaction_speculation {
            Speculation::Running => {
                if ctx.speculation_blink_on {
                    accent_icon
                } else {
                    theme.fg("muted", &auto)
                }
            }
            Speculation::Armed => accent_icon,
            Speculation::Idle => theme.fg(color, &auto),
        };
        auto_icon = format!(" {styled}");
    }

    let text = if percent.is_none() && window > 0 {
        format_number(window as f64)
    } else {
        format_context_usage(percent, window, ctx.context_tokens)
    };
    let text = theme.fg(color, &text);
    let content = with_icon(&theme.symbol("icon.context"), &format!("{text}{auto_icon}"));
    RenderedSegment::shown(content)
}

fn context_total_segment(theme: &dyn StatusTheme, ctx: &SegmentContext) -> RenderedSegment {
    if ctx.context_window == 0 {
        return RenderedSegment::hidden();
    }
    let content = with_icon(
        &theme.symbol("icon.context"),
        &format_number(ctx.context_window as f64),
    );
    RenderedSegment::shown(theme.fg("statusLineContext", &content))
}

fn time_spent_segment(theme: &dyn StatusTheme, ctx: &SegmentContext) -> RenderedSegment {
    if ctx.active_ms < 1_000 {
        return RenderedSegment::hidden();
    }
    let content = with_icon(&theme.symbol("icon.time"), &format_duration(ctx.active_ms));
    RenderedSegment::shown(content)
}

fn format_clock(ctx: &SegmentContext) -> String {
    let opts = ctx.options.time.clone().unwrap_or_default();
    let clock = ctx.now.unwrap_or_default();
    let mut hours = clock.hour;
    let mut suffix = String::new();
    if opts.format() == TimeFormat::H12 {
        suffix = if hours >= 12 { "pm" } else { "am" }.to_string();
        hours = if hours % 12 == 0 { 12 } else { hours % 12 };
    }
    let mut text = format!("{}:{:02}", hours, clock.minute);
    if opts.show_seconds() {
        text.push_str(&format!(":{:02}", clock.second));
    }
    text.push_str(&suffix);
    text
}

fn time_segment(theme: &dyn StatusTheme, ctx: &SegmentContext) -> RenderedSegment {
    let content = with_icon(&theme.symbol("icon.time"), &format_clock(ctx));
    RenderedSegment::shown(content)
}

fn session_segment(theme: &dyn StatusTheme, ctx: &SegmentContext) -> RenderedSegment {
    let display = ctx
        .session
        .session_id
        .as_ref()
        .map(|id| id.chars().take(8).collect::<String>())
        .filter(|id| !id.is_empty())
        .unwrap_or_else(|| "new".to_string());
    let content = with_icon(&theme.symbol("icon.session"), &display);
    RenderedSegment::shown(content)
}

fn hostname_segment(theme: &dyn StatusTheme, ctx: &SegmentContext) -> RenderedSegment {
    let name = match &ctx.hostname {
        Some(name) if !name.is_empty() => name.clone(),
        _ => return RenderedSegment::hidden(),
    };
    let content = with_icon(&theme.symbol("icon.host"), &name);
    let ansi = session_accent_ansi(theme, ctx);
    match ansi {
        Some(ansi) => RenderedSegment::shown(format!("{ansi}{content}\x1b[39m")),
        None => RenderedSegment::shown(content),
    }
}

fn cache_hit_segment(theme: &dyn StatusTheme, ctx: &SegmentContext) -> RenderedSegment {
    let stats = &ctx.usage_stats;
    if stats.cache_read == 0 {
        return RenderedSegment::hidden();
    }
    let total = stats.cache_read + stats.cache_write + stats.input;
    let rate = if total == 0 {
        0.0
    } else {
        (stats.cache_read as f64 / total as f64) * 100.0
    };
    let content = format!(
        "{} {}",
        theme.symbol("icon.cache"),
        theme.fg("statusLineSpend", &format!("{rate:.2}%"))
    );
    RenderedSegment::shown(content)
}

fn cache_segment(theme: &dyn StatusTheme, ctx: &SegmentContext) -> RenderedSegment {
    let status = match &ctx.prompt_cache {
        Some(status) => status,
        None => return RenderedSegment::hidden(),
    };
    let content = match status.state {
        PromptCacheState::Warm => theme.fg("dim", &theme.symbol("icon.cacheWarm")),
        PromptCacheState::Cold => theme.fg("warning", &theme.symbol("icon.cacheCold")),
    };
    RenderedSegment::shown(content)
}

fn session_name_segment(theme: &dyn StatusTheme, ctx: &SegmentContext) -> RenderedSegment {
    let name = ctx
        .session
        .session_name
        .clone()
        .or_else(|| ctx.preview_title.clone());
    let name = match name {
        Some(name) => name,
        None => return RenderedSegment::hidden(),
    };
    let styled_title = accent_fg(theme, ctx, "accent", &sanitize_status_text(&name));
    let pin = theme.symbol("icon.pin");
    let content = if ctx.is_pinned && !pin.is_empty() {
        format!("{} {}", theme.fg("accent", &pin), styled_title)
    } else {
        styled_title
    };
    RenderedSegment::shown(content)
}

fn collab_segment(theme: &dyn StatusTheme, ctx: &SegmentContext) -> RenderedSegment {
    let collab = match &ctx.collab {
        Some(collab) => collab,
        None => return RenderedSegment::hidden(),
    };
    let label = match collab.role {
        CollabRole::Host => format!("⇄ collab:{}", collab.participant_count),
        CollabRole::Guest => format!("⇄ collab guest:{}", collab.participant_count),
    };
    RenderedSegment::shown(accent_fg(theme, ctx, "accent", &label))
}

fn stream_segment(theme: &dyn StatusTheme, ctx: &SegmentContext) -> RenderedSegment {
    let mut badges: Vec<String> = Vec::new();
    if let Some(stream) = &ctx.stream {
        badges.push(format!("● LIVE {}", stream.viewers));
    }
    if ctx.recording {
        badges.push("● REC".to_string());
    }
    if badges.is_empty() {
        return RenderedSegment::hidden();
    }
    RenderedSegment::shown(theme.fg("thinkingHigh", &badges.join(" ")))
}

fn vim_segment(theme: &dyn StatusTheme, ctx: &SegmentContext) -> RenderedSegment {
    let vim = match &ctx.vim {
        Some(vim) => vim,
        None => return RenderedSegment::hidden(),
    };
    if vim.display == VimDisplay::None {
        return RenderedSegment::hidden();
    }
    let mut label = match vim.display {
        VimDisplay::Icon => theme.symbol(vim.mode.icon_key()),
        _ => vim.mode.label().to_string(),
    };
    if vim.selected_lines > 1 {
        label.push_str(&format!(" {}L", vim.selected_lines));
    }
    let mut content = theme.fg(vim.mode.color(), &label);
    if !vim.pending.is_empty() {
        content.push_str(&theme.fg("muted", &format!(" {}", vim.pending)));
    }
    RenderedSegment::shown(content)
}

fn format_quota_window(
    theme: &dyn StatusTheme,
    label: &str,
    percent: f64,
    reset: Option<u64>,
    reset_unit: char,
    integer: &str,
) -> String {
    let whole = if integer == "floor" {
        percent.floor()
    } else {
        percent.round()
    } as i64;
    let pct_text = theme.fg(pick_usage_color(percent), &format!("{whole}%"));
    let reset_text = reset.map(|value| {
        theme.fg("muted", &format!(" ({})", format_usage_reset(value, reset_unit)))
    });
    format!("{label} {pct_text}{}", reset_text.unwrap_or_default())
}

fn usage_segment(theme: &dyn StatusTheme, ctx: &SegmentContext) -> RenderedSegment {
    let usage = match &ctx.usage {
        Some(usage) => usage,
        None => return RenderedSegment::hidden(),
    };
    if usage.five_hour.is_none()
        && usage.daily.is_none()
        && usage.seven_day.is_none()
        && usage.monthly.is_none()
        && usage.reset_credits.is_none()
    {
        return RenderedSegment::hidden();
    }

    let mut parts: Vec<String> = Vec::new();
    if let Some(tier) = &usage.tier {
            let tier = truncate_to_width(&sanitize_status_text(tier), 40, Ellipsis::Unicode);
        if !tier.is_empty() {
            parts.push(accent_fg(theme, ctx, "accent", &tier));
        }
    }
    if let Some(window) = usage.five_hour {
        parts.push(format_quota_window(
            theme,
            "5h",
            window.percent,
            window.reset_minutes,
            'm',
            "round",
        ));
    }
    if let Some(window) = usage.daily {
        parts.push(format_quota_window(
            theme,
            "1d",
            window.percent,
            window.reset_minutes,
            'm',
            "round",
        ));
    }
    if let Some(window) = usage.seven_day {
        parts.push(format_quota_window(
            theme,
            "7d",
            window.percent,
            window.reset_hours,
            'h',
            "round",
        ));
    }
    if let Some(window) = usage.monthly {
        parts.push(format_quota_window(
            theme,
            "mo",
            window.percent,
            window.reset_hours,
            'h',
            "floor",
        ));
    }
    if let Some(resets) = &usage.reset_credits {
        let mut reset_text = format!("✦ {}", resets.banked_count);
        if resets.redeemable_count != resets.banked_count {
            reset_text.push_str(&format!(" ({} usable)", resets.redeemable_count));
        }
        if let Some(hours) = resets.expiry_hours {
            let expiring = match &resets.expiring {
                Some(expiring) => format!("▲ {} ", expiring.count),
                None => String::new(),
            };
            reset_text.push_str(&format!(
                " {expiring}exp {}",
                format_usage_reset(hours, 'h')
            ));
        } else if resets.expired {
            reset_text.push_str(" expired");
        }
        if resets.redeemable_count == 0 {
            if let Some(reason) = &resets.unavailable_reason {
                    let reason = truncate_to_width(&sanitize_status_text(reason), 40, Ellipsis::Unicode);
                if !reason.is_empty() {
                    reset_text.push_str(&format!(" {reason}"));
                }
            }
        }
        parts.push(theme.fg(reset_credits_color(resets), &reset_text));
    }

    let content = with_icon(&theme.symbol("icon.time"), &parts.join(&theme.sep("dot")));
    RenderedSegment::shown(content)
}

// ═══════════════════════════════════════════════════════════════════════════
// Segment Registry
// ═══════════════════════════════════════════════════════════════════════════

/// Render one catalog segment.
///
/// Unknown identifiers render hidden; the catalog is closed, so this only
/// happens for a caller-supplied id that failed validation.
pub fn render_segment(id: SegmentId, ctx: &SegmentContext) -> RenderedSegment {
    let theme = ActiveTheme::active();
    render_segment_with(id, &*theme, ctx)
}

/// Render one catalog segment against an explicit theme.
pub fn render_segment_with(id: SegmentId, theme: &dyn StatusTheme, ctx: &SegmentContext) -> RenderedSegment {
    match id {
        SegmentId::Pi => pi_segment(theme, ctx),
        SegmentId::Status => status_segment(theme, ctx),
        SegmentId::Model => model_segment(theme, ctx),
        SegmentId::Mode => mode_segment(theme, ctx),
        SegmentId::Path => path_segment(theme, ctx),
        SegmentId::Git => git_segment(theme, ctx),
        SegmentId::Pr => pr_segment(theme, ctx),
        SegmentId::Subagents => subagents_segment(theme, ctx),
        SegmentId::TokenIn => single_stat_segment(
            "input",
            "icon.input",
            "statusLineSpend",
            theme,
            ctx,
        ),
        SegmentId::TokenOut => single_stat_segment(
            "output",
            "icon.output",
            "statusLineOutput",
            theme,
            ctx,
        ),
        SegmentId::TokenTotal => token_total_segment(theme, ctx),
        SegmentId::TokenRate => token_rate_segment(theme, ctx),
        SegmentId::Cost => cost_segment(theme, ctx),
        SegmentId::ContextPct => context_pct_segment(theme, ctx),
        SegmentId::ContextTotal => context_total_segment(theme, ctx),
        SegmentId::TimeSpent => time_spent_segment(theme, ctx),
        SegmentId::Time => time_segment(theme, ctx),
        SegmentId::Session => session_segment(theme, ctx),
        SegmentId::Hostname => hostname_segment(theme, ctx),
        SegmentId::CacheRead => single_stat_segment(
            "cache_read",
            "icon.cache",
            "statusLineSpend",
            theme,
            ctx,
        ),
        SegmentId::CacheWrite => single_stat_segment(
            "cache_write",
            "icon.cache",
            "statusLineOutput",
            theme,
            ctx,
        ),
        SegmentId::CacheHit => cache_hit_segment(theme, ctx),
        SegmentId::Cache => cache_segment(theme, ctx),
        SegmentId::SessionName => session_name_segment(theme, ctx),
        SegmentId::Usage => usage_segment(theme, ctx),
        SegmentId::Collab => collab_segment(theme, ctx),
        SegmentId::Stream => stream_segment(theme, ctx),
        SegmentId::Vim => vim_segment(theme, ctx),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::status_line::FallbackTheme;
    use crate::status_line::schema::{PathOptions, TimeFormat, TimeOptions};

    /// Colorless theme over the Unicode glyph set, so exact strings stay readable.
    struct PlainTheme;

    impl StatusTheme for PlainTheme {
        fn fg_ansi(&self, _token: &str) -> String {
            String::new()
        }

        fn bg_ansi(&self, _token: &str) -> String {
            String::new()
        }

        fn symbol(&self, key: &str) -> String {
            FallbackTheme::unicode_symbol(key).to_string()
        }

        fn sep(&self, name: &str) -> String {
            FallbackTheme::new().sep(name)
        }

        fn horizontal(&self) -> String {
            "─".to_string()
        }

        fn spinner_frames(&self, kind: &str) -> Vec<String> {
            FallbackTheme::new().spinner_frames(kind)
        }
    }

    fn sample_ctx() -> SegmentContext { let mut ctx = SegmentContext::default();
    ctx.project_dir = "/home/u/proj".to_string();
    ctx.home_dir = "/home/u".to_string();
    ctx.now = Some(SegmentClock {
        hour: 13,
        minute: 5,
        second: 9,
        epoch_ms: 0,
    });
    ctx.spinner_interval_ms = 80;
    ctx }

    fn render(id: SegmentId, ctx: &SegmentContext) -> RenderedSegment {
        render_segment_with(id, &PlainTheme, ctx)
    }

    #[test]
    fn model_segment_shows_name_and_thinking_tail() {
        let mut ctx = sample_ctx();
        ctx.session.model = ModelInfo {
            name: Some("Claude Sonnet".to_string()),
            thinking: true,
            thinking_level: Some("high".to_string()),
            ..ModelInfo::default()
        };
        let segment = render(SegmentId::Model, &ctx);
        assert!(segment.visible);
        assert_eq!(segment.content, "⬢ Sonnet\x1b[39m · ◒ high\x1b[39m");
    }

    #[test]
    fn model_segment_falls_back_to_id_then_placeholder() {
        let mut ctx = sample_ctx();
        ctx.session.model = ModelInfo {
            id: Some("glm-5".to_string()),
            ..ModelInfo::default()
        };
        assert_eq!(render(SegmentId::Model, &ctx).content, "⬢ glm-5\x1b[39m");

        let mut ctx = sample_ctx();
        ctx.session.model.name = Some(String::new());
        assert_eq!(render(SegmentId::Model, &ctx).content, "⬢ no-model\x1b[39m");
    }

    #[test]
    fn path_segment_abbreviates_home() {
        let ctx = sample_ctx();
        let segment = render(SegmentId::Path, &ctx);
        assert_eq!(segment.content, "📁 ~/proj\x1b[39m");
    }

    #[test]
    fn path_segment_clamps_to_max_length() {
        let mut ctx = sample_ctx();
        ctx.options.path = Some(PathOptions {
            max_length: Some(4),
            ..PathOptions::default()
        });
        let segment = render(SegmentId::Path, &ctx);
        assert_eq!(segment.content, "📁 …roj\x1b[39m");
    }

    #[test]
    fn git_segment_clean_and_dirty() {
        let mut ctx = sample_ctx();
        ctx.git.branch = Some("main".to_string());
        ctx.git.status = Some(GitStatus {
            staged: 0,
            unstaged: 0,
            untracked: 0,
        });
        assert_eq!(render(SegmentId::Git, &ctx).content, "⑂ main\x1b[39m");

        ctx.git.status = Some(GitStatus {
            unstaged: 2,
            ..GitStatus::default()
        });
        // The indicator is painted with its own token first, so its reset rides
        // inside the segment's closing reset — exactly as `gitSegment.render`.
        assert_eq!(render(SegmentId::Git, &ctx).content, "⑂ main *2\x1b[39m\x1b[39m");
    }

    #[test]
    fn git_segment_hidden_outside_repository() {
        let ctx = sample_ctx();
        let segment = render(SegmentId::Git, &ctx);
        assert!(!segment.visible);
        assert!(segment.content.is_empty());
    }

    #[test]
    fn token_total_excludes_cache_read() {
        let mut ctx = sample_ctx();
        ctx.usage_stats.input = 100;
        ctx.usage_stats.output = 50;
        ctx.usage_stats.cache_read = 100_000;
        assert_eq!(render(SegmentId::TokenTotal, &ctx).content, "🪙 150\x1b[39m");
        assert!(!render(SegmentId::TokenIn, &ctx).content.is_empty());
    }

    #[test]
    fn cost_segment_formats_dollars_and_subagents() {
        let mut ctx = sample_ctx();
        ctx.usage_stats.cost = 2.0;
        ctx.usage_stats.subagent_cost = Some(0.5);
        ctx.subagent_tree_cost = Some(1.0);
        assert_eq!(render(SegmentId::Cost, &ctx).content, "$1.50 (+1.00)\x1b[39m");
    }

    #[test]
    fn cost_segment_hidden_without_spend() {
        let ctx = sample_ctx();
        assert!(!render(SegmentId::Cost, &ctx).visible);
    }

    #[test]
    fn context_pct_formats_percent_and_window() {
        let mut ctx = sample_ctx();
        ctx.context_percent = Some(50.0);
        ctx.context_window = 200_000;
        assert_eq!(render(SegmentId::ContextPct, &ctx).content, "◫ 50.0%/200K\x1b[39m");
    }

    #[test]
    fn context_pct_shows_window_when_percent_unknown() {
        let mut ctx = sample_ctx();
        ctx.context_percent = None;
        ctx.context_window = 200_000;
        assert_eq!(render(SegmentId::ContextPct, &ctx).content, "◫ 200K\x1b[39m");
    }

    #[test]
    fn context_total_hidden_without_window() {
        let ctx = sample_ctx();
        assert!(!render(SegmentId::ContextTotal, &ctx).visible);
    }

    #[test]
    fn time_segment_24h_and_12h() {
        let ctx = sample_ctx();
        assert_eq!(render(SegmentId::Time, &ctx).content, "⏱ 13:05");

        let mut ctx = sample_ctx();
        ctx.options.time = Some(TimeOptions {
            format: Some(TimeFormat::H12),
            show_seconds: Some(true),
        });
        assert_eq!(render(SegmentId::Time, &ctx).content, "⏱ 1:05:09pm");
    }

    #[test]
    fn session_segment_prefixes_id_or_new() {
        let mut ctx = sample_ctx();
        ctx.session.session_id = Some("abcdef123456".to_string());
        assert_eq!(render(SegmentId::Session, &ctx).content, "🆔 abcdef12");

        let ctx = sample_ctx();
        assert_eq!(render(SegmentId::Session, &ctx).content, "🆔 new");
    }

    #[test]
    fn hostname_segment_hidden_without_host() {
        let ctx = sample_ctx();
        assert!(!render(SegmentId::Hostname, &ctx).visible);

        let mut ctx = sample_ctx();
        ctx.hostname = Some("mac".to_string());
        assert_eq!(render(SegmentId::Hostname, &ctx).content, "🖥 mac");
    }

    #[test]
    fn session_name_uses_preview_title() {
        let mut ctx = sample_ctx();
        ctx.preview_title = Some("Ship it".to_string());
        assert_eq!(render(SegmentId::SessionName, &ctx).content, "Ship it\x1b[39m");
    }

    #[test]
    fn vim_segment_text_and_icon() {
        let mut ctx = sample_ctx();
        ctx.vim = Some(VimState {
            mode: VimMode::Normal,
            display: VimDisplay::Text,
            selected_lines: 3,
            ..VimState::default()
        });
        assert_eq!(render(SegmentId::Vim, &ctx).content, "NORMAL 3L\x1b[39m");

        let mut ctx = sample_ctx();
        ctx.vim = Some(VimState {
            mode: VimMode::Insert,
            display: VimDisplay::Icon,
            pending: "2d".to_string(),
            ..VimState::default()
        });
        assert_eq!(
            render(SegmentId::Vim, &ctx).content,
            "▎\x1b[39m 2d\x1b[39m"
        );
    }

    #[test]
    fn usage_segment_hidden_without_quota() {
        let ctx = sample_ctx();
        assert!(!render(SegmentId::Usage, &ctx).visible);
    }

    #[test]
    fn usage_segment_formats_quota_windows() {
        let mut ctx = sample_ctx();
        ctx.usage = Some(UsageInfo {
            five_hour: Some(QuotaWindow {
                percent: 81.0,
                reset_minutes: Some(45),
                reset_hours: None,
            }),
            monthly: Some(QuotaWindow {
                percent: 12.4,
                reset_minutes: None,
                reset_hours: Some(500),
            }),
            ..UsageInfo::default()
        });
        let content = render(SegmentId::Usage, &ctx).content;
        assert!(content.starts_with("⏱ "), "{content}");
        assert!(content.contains("5h 81%"), "{content}");
        assert!(content.contains("mo 12%"), "{content}");
    }

    #[test]
    fn pi_segment_spinner_and_timer() {
        let mut ctx = sample_ctx();
        ctx.turn_elapsed_ms = Some(65_000);
        ctx.session.profile = Some("work".to_string());
        assert_eq!(render(SegmentId::Pi, &ctx).content, "⠋-work 1m\x1b[39m");
    }

    #[test]
    fn pi_segment_ghost_for_focused_agent() {
        let mut ctx = sample_ctx();
        ctx.focused_agent_id = Some("agent-7".to_string());
        assert_eq!(
            render(SegmentId::Pi, &ctx).content,
            "👻 agent-7\x1b[39m"
        );
    }

    #[test]
    fn status_segment_joins_with_dot() {
        let mut ctx = sample_ctx();
        ctx.hook_statuses = vec!["one".to_string(), "two".to_string()];
        assert_eq!(render(SegmentId::Status, &ctx).content, "one · two\x1b[39m");
    }

    #[test]
    fn mode_segment_loop_and_goal() {
        let mut ctx = sample_ctx();
        ctx.loop_mode = Some(LoopMode {
            state: LoopState::Waiting,
            limit: Some(LoopLimit::Iterations {
                initial: 5,
                remaining: 2,
            }),
            condition: None,
        });
        assert_eq!(
            render(SegmentId::Mode, &ctx).content,
            "↻ Loop: next prompt repeats 2/5\x1b[39m"
        );

        let mut ctx = sample_ctx();
        ctx.goal_mode = Some(PausedMode {
            enabled: true,
            paused: false,
        });
        assert_eq!(render(SegmentId::Mode, &ctx).content, "🎯 Goal\x1b[39m");
    }

    #[test]
    fn format_number_suffixes() {
        assert_eq!(format_number(999.0), "999");
        assert_eq!(format_number(1_000.0), "1K");
        assert_eq!(format_number(1_500.0), "1.5K");
        assert_eq!(format_number(25_000.0), "25K");
        assert_eq!(format_number(1_000_000.0), "1M");
        assert_eq!(format_number(1_500_000.0), "1.5M");
        assert_eq!(format_number(25_000_000.0), "25M");
        assert_eq!(format_number(1_000_000_000.0), "1B");
        assert_eq!(format_number(1_500_000_000.0), "1.5B");
    }

    #[test]
    fn format_duration_units() {
        assert_eq!(format_duration(0), "0ms");
        assert_eq!(format_duration(123), "123ms");
        assert_eq!(format_duration(1_500), "1.5s");
        assert_eq!(format_duration(59_950), "1m");
        assert_eq!(format_duration(30_000), "30.0s");
        assert_eq!(format_duration(90_000), "1m30s");
        assert_eq!(format_duration(3_600_000), "1h");
        assert_eq!(format_duration(90_000_000), "1d1h");
    }

    #[test]
    fn format_context_usage_variants() {
        assert_eq!(format_context_usage(Some(50.0), 200_000, 0), "50.0%/200K");
        assert_eq!(format_context_usage(None, 200_000, 5), "?/200K");
        assert_eq!(format_context_usage(None, 0, 5), "5/?");
    }

    #[test]
    fn context_usage_levels() {
        assert_eq!(context_usage_level(50.0, 200_000), ContextUsageLevel::Warning);
        assert_eq!(context_usage_level(75.0, 200_000), ContextUsageLevel::Purple);
        assert_eq!(context_usage_level(95.0, 200_000), ContextUsageLevel::Error);
        assert_eq!(context_usage_level(10.0, 200_000), ContextUsageLevel::Normal);
        assert_eq!(context_usage_level(0.0, 200_000), ContextUsageLevel::Normal);
    }

    #[test]
    fn sanitize_collapses_whitespace_and_strips_escapes() {
        assert_eq!(sanitize_status_text("  a \n\t b  "), "a b");
        assert_eq!(sanitize_status_text("\x1b[31mred\x1b[0m"), "red");
        assert_eq!(sanitize_status_text(""), "");
    }

    #[test]
    fn clamp_path_length_keeps_tail() {
        assert_eq!(clamp_path_length("abcdefgh", 4), "…fgh");
        assert_eq!(clamp_path_length("abc", 4), "abc");
    }

    #[test]
    fn shorten_path_replaces_home() {
        assert_eq!(shorten_path("/home/u/proj", "/home/u"), "~/proj");
        assert_eq!(shorten_path("/home/u", "/home/u"), "~");
        assert_eq!(shorten_path("/etc/hosts", "/home/u"), "/etc/hosts");
    }

    #[test]
    fn with_icon_omits_empty_icon() {
        assert_eq!(with_icon("", "x"), "x");
        assert_eq!(with_icon("⬢", "x"), "⬢ x");
    }

    #[test]
    fn billing_summary_subscription_and_advisor() {
        let mut ctx = sample_ctx();
        ctx.session.using_subscription = true;
        ctx.usage_stats.cost = 1.234;
        ctx.session.advisor_cost = 0.4;
        let summary = billing_summary(&ctx, &PlainTheme).unwrap();
        assert_eq!(summary, "S1.23 + 👁 $0.40");
    }

    #[test]
    fn loop_limit_duration_counts_down() {
        let limit = LoopLimit::Duration {
            deadline_ms: 90_000,
        };
        assert_eq!(format_loop_limit(&limit, 0).unwrap(), "1m30s left");
        let limit = LoopLimit::Iterations {
            initial: 5,
            remaining: 2,
        };
        assert_eq!(format_loop_limit(&limit, 0).unwrap(), "2/5");
    }

    #[test]
    fn render_segment_dispatches_catalog() {
        let ctx = sample_ctx();
        assert!(render(SegmentId::Model, &ctx).visible);
        assert!(!render(SegmentId::Git, &ctx).visible);
    }
}
