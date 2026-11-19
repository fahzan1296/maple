//! Reference: `l/src/overlays/session-selector.ts`.
//!
//! The resume picker: a searchable session list grouped by day, pinned first,
//! with a native picker preview, delete confirmation and inline rename.
//!
//! Deviations:
//! - `Promise`/`setTimeout` host work (complete listings, history merge
//!   debounce, preview settle, chunked fuzzy scan) becomes synchronous
//!   callbacks: [`SessionSelectorOptions::load_all_sessions`],
//!   [`SessionHistoryMatcher`] and the `on_*` actions all return `Result`.
//!   Fuzzy scoring runs the whole listing in one pass through
//!   [`crate::fuzzy::fuzzy_match`], which is fast enough at these sizes and
//!   keeps ranking deterministic under tests.
//! - Per-object symbol caches (`kNativeItem`, `kSearchTextLower`) become
//!   per-call computation; the port favours plain data over hidden caches.
//! - The ANSI multiline list reuses the crate's `Input` for the query line and
//!   a `ScrollView` for the windowed rows, matching the reference's layout.
//! - `Intl.DateTimeFormat.supportedLocalesOf` has no Rust counterpart; a tag is
//!   accepted when it looks like a BCP 47 tag and is not `C`/`POSIX`, and date
//!   formatting falls back to `YYYY-MM-DD` for week-old sessions.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use crate::chrome::overlay_box::{bottom_border, row, top_border, OverlayTheme};
use crate::chrome::selector_helpers::content_row_width;
use crate::component::Component;
use crate::components::input::Input;
use crate::components::menu_selection::{
	get_menu_window, MenuAlignment, MenuWindowOptions,
};
use crate::components::scroll_view::ScrollView;
use crate::fuzzy::fuzzy_match;
use crate::keys::Key;
use crate::native::describe::{kv, md, node, span, text};
use crate::native::node::{DescribeContext, NativeNode, NodeKind, NodeText, Props};
use crate::native::overlay::{action_hint, hints_row, overlay_card};
use crate::native::picker::{
	PickerActionOptions, PickerColumn, PickerConfirm, PickerItem, PickerProps,
	picker_action, picker_age, picker_date, picker_hits,
};
use crate::native::backend::NativeUiEvent;
use crate::native::NativeDescribe;
use crate::overlays::plugin_settings::shorten_path;
use crate::render::render_utils::padding;
use crate::text_shape::{Ellipsis, pad_to_width, truncate_to_width, visible_width};
use crate::theme::theme_class::Theme;
use crate::tools::output_meta::format_bytes;

/// Session lifecycle status presented by the picker.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SessionSelectorStatus {
	/// Finished normally.
	Complete,
	/// Interrupted by the user.
	Interrupted,
	/// Aborted.
	Aborted,
	/// Failed.
	Error,
	/// Still running.
	Pending,
	/// Unknown / unset.
	#[default]
	Unknown,
}

impl SessionSelectorStatus {
	#[must_use]
	pub fn as_str(self) -> &'static str {
		match self {
			Self::Complete => "complete",
			Self::Interrupted => "interrupted",
			Self::Aborted => "aborted",
			Self::Error => "error",
			Self::Pending => "pending",
			Self::Unknown => "unknown",
		}
	}

	/// Parse a stored status string.
	#[must_use]
	pub fn parse(value: &str) -> Self {
		match value {
			"complete" => Self::Complete,
			"interrupted" => Self::Interrupted,
			"aborted" => Self::Aborted,
			"error" => Self::Error,
			"pending" => Self::Pending,
			_ => Self::Unknown,
		}
	}
}

/// Path to the parent session when this one was forked.
pub type SessionParentPath = Option<String>;

/// Session listing fields consumed by the picker.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SessionSelectorEntry {
	/// Session file path.
	pub path: String,
	/// Session id.
	pub id: String,
	/// Working directory the session ran in.
	pub cwd: String,
	/// Display title, when the session has one.
	pub title: Option<String>,
	/// Last modification, epoch milliseconds.
	pub modified: i64,
	/// Creation time, epoch milliseconds.
	pub created: Option<i64>,
	/// Session file size in bytes.
	pub size: u64,
	/// First user message.
	pub first_message: String,
	/// All message text, used for search and the preview's tail.
	pub all_messages_text: String,
	/// Lifecycle status.
	pub status: Option<SessionSelectorStatus>,
	/// Path to the parent session when this one was forked.
	pub parent_session_path: SessionParentPath,
}

impl SessionSelectorEntry {
	/// A minimal entry, for hosts and tests.
	#[must_use]
	pub fn new(path: impl Into<String>, id: impl Into<String>, modified: i64) -> Self {
		SessionSelectorEntry {
			path: path.into(),
			id: id.into(),
			modified,
			..SessionSelectorEntry::default()
		}
	}
}

impl AsRef<SessionSelectorEntry> for SessionSelectorEntry {
	fn as_ref(&self) -> &Self {
		self
	}
}
impl AsMut<SessionSelectorEntry> for SessionSelectorEntry {
	fn as_mut(&mut self) -> &mut Self {
		self
	}
}

/// Returns the IDs of sessions whose recorded prompts match a query, best
/// first.
pub type SessionHistoryMatcher = Box<dyn FnMut(&str) -> Vec<String>>;

/// A cwd as dim-prefix / strong-tail path spans.
#[must_use]
pub fn cwd_spans(cwd: &str) -> Vec<crate::native::node::Span> {
	let short = shorten_path(cwd);
	let cut = short.rfind('/').map_or(0, |index| index + 1);
	if cut > 0 && cut < short.len() {
		vec![
			span(&short[..cut], Some("path dim")),
			span(&short[cut..], Some("path")),
		]
	} else {
		vec![span(short, Some("path"))]
	}
}

/// Themed glyph plus label for a session's lifecycle status, or `None` when
/// there is nothing useful to show.
#[must_use]
pub fn format_session_status(theme: &Theme, status: Option<SessionSelectorStatus>) -> Option<String> {
	let status = status?;
	let (tone, glyph, label) = match status {
		SessionSelectorStatus::Complete => ("success", "✓", "done"),
		SessionSelectorStatus::Interrupted => ("warning", "!", "interrupted"),
		SessionSelectorStatus::Aborted => ("muted", "×", "aborted"),
		SessionSelectorStatus::Error => ("error", "✗", "error"),
		SessionSelectorStatus::Pending => ("accent", "…", "pending"),
		SessionSelectorStatus::Unknown => return None,
	};
	Some(theme.fg(tone, &format!("{glyph} {label}")).ok()?)
}

/// Lifecycle status as one themed span, mirroring [`format_session_status`].
#[must_use]
pub fn session_status_span(status: Option<SessionSelectorStatus>) -> Option<crate::native::node::Span> {
	let (tone, label) = match status? {
		SessionSelectorStatus::Complete => ("success", "done"),
		SessionSelectorStatus::Interrupted => ("warning", "interrupted"),
		SessionSelectorStatus::Aborted => ("muted", "aborted"),
		SessionSelectorStatus::Error => ("error", "error"),
		SessionSelectorStatus::Pending => ("accent", "pending"),
		SessionSelectorStatus::Unknown => return None,
	};
	Some(span(label, Some(tone)))
}

/// Status dot tone of a session row in the native picker.
#[must_use]
pub fn session_status_tone(status: Option<SessionSelectorStatus>) -> Option<&'static str> {
	match status? {
		SessionSelectorStatus::Complete => Some("success"),
		SessionSelectorStatus::Interrupted => Some("warning"),
		SessionSelectorStatus::Aborted | SessionSelectorStatus::Error => Some("error"),
		SessionSelectorStatus::Pending => Some("pending"),
		SessionSelectorStatus::Unknown => None,
	}
}

/// The name a session goes by: its title, else its first message on one line,
/// else its id.
#[must_use]
pub fn session_label(session: &SessionSelectorEntry) -> String {
	let title = session
		.title
		.as_deref()
		.map(|title| collapse_whitespace(title).trim().to_string())
		.unwrap_or_default();
	if !title.is_empty() {
		return title;
	}
	let message = collapse_whitespace(&session.first_message).trim().to_string();
	if !message.is_empty() {
		return message;
	}
	session.id.clone()
}

/// Collapse every whitespace run to one space.
#[must_use]
pub fn collapse_whitespace(text: &str) -> String {
	let mut out = String::with_capacity(text.len());
	let mut space = false;
	for ch in text.chars() {
		if ch.is_whitespace() {
			if !space {
				out.push(' ');
				space = true;
			}
		} else {
			out.push(ch);
			space = false;
		}
	}
	out
}

/// Longest prompt or answer excerpt the native preview shows.
pub const PREVIEW_EXCERPT_CHARS: usize = 1200;
/// The preview follows the selection once it rests this long.
pub const PREVIEW_SETTLE_MS: u64 = 60;
/// Milliseconds in a day.
pub const DAY_MS: i64 = 86_400_000;
/// Group id and label of pinned sessions.
pub const PINNED_GROUP: (&str, &str) = ("pinned", "Pinned");

/// Day group of a session's last modification.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionDayGroup {
	/// Group id (`today`, `yesterday`, `week`, `earlier`).
	pub id: &'static str,
	/// Group label.
	pub label: &'static str,
}

/// Day group of a session's last modification, relative to local midnight
/// `start_of_today` (epoch ms).
#[must_use]
pub fn session_day_group(modified: i64, start_of_today: i64) -> SessionDayGroup {
	if modified >= start_of_today {
		SessionDayGroup { id: "today", label: "Today" }
	} else if modified >= start_of_today - DAY_MS {
		SessionDayGroup { id: "yesterday", label: "Yesterday" }
	} else if modified >= start_of_today - 6 * DAY_MS {
		SessionDayGroup { id: "week", label: "This week" }
	} else {
		SessionDayGroup { id: "earlier", label: "Earlier" }
	}
}

/// BCP 47 tag from the POSIX locale variables (`LC_ALL`, then `LC_TIME`, then
/// `LANG`), or `None` for the runtime default.
#[must_use]
pub fn system_date_locale(env: &HashMap<String, String>) -> Option<String> {
	let raw = env
		.get("LC_ALL")
		.or_else(|| env.get("LC_TIME"))
		.or_else(|| env.get("LANG"))?;
	let tag = raw
		.split(['.', '@'])
		.next()
		.unwrap_or("")
		.replace('_', "-");
	if tag.is_empty() || tag == "C" || tag == "POSIX" {
		return None;
	}
	let language = tag.split('-').next().unwrap_or("");
	let looks_like_tag = (2..=3).contains(&language.len())
		&& language.chars().all(|ch| ch.is_ascii_alphabetic());
	looks_like_tag.then_some(tag)
}

/// Relative age of a session's last modification (`"3 hours ago"`), falling
/// back to the date after a week.
#[must_use]
pub fn format_session_date(modified: i64, now: i64) -> String {
	let diff_ms = now - modified;
	let diff_mins = diff_ms / 60_000;
	let diff_hours = diff_ms / 3_600_000;
	let diff_days = diff_ms / DAY_MS;
	if diff_mins < 1 {
		return "just now".to_string();
	}
	if diff_mins < 60 {
		return format!(
			"{diff_mins} minute{} ago",
			if diff_mins != 1 { "s" } else { "" }
		);
	}
	if diff_hours < 24 {
		return format!(
			"{diff_hours} hour{} ago",
			if diff_hours != 1 { "s" } else { "" }
		);
	}
	if diff_days == 1 {
		return "1 day ago".to_string();
	}
	if diff_days < 7 {
		return format!("{diff_days} days ago");
	}
	format_iso_date(modified)
}

/// `YYYY-MM-DD` (UTC) for an epoch-ms timestamp.
fn format_iso_date(at: i64) -> String {
	let days = at.div_euclid(DAY_MS);
	let (year, month, day) = civil_from_days(days);
	format!("{year:04}-{month:02}-{day:02}")
}

/// Howard Hinnant's days-to-civil conversion.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
	let z = days + 719_468;
	let era = z.div_euclid(146_097);
	let doe = z - era * 146_097;
	let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
	let y = yoe + era * 400;
	let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
	let mp = (5 * doy + 2) / 153;
	let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
	let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
	(if m <= 2 { y + 1 } else { y }, m, d)
}

/// Month/day label for a picker fact column.
#[must_use]
pub fn picker_day_label(at: i64) -> String {
	let days = at.div_euclid(DAY_MS);
	let (_, month, day) = civil_from_days(days);
	format!("{month}/{day}")
}

/// The native preview of a session: its title, a fact grid, and the
/// conversation's first prompt plus the tail of the rest as an excerpt of the
/// latest answer.
#[must_use]
pub fn session_preview(
	session: &SessionSelectorEntry,
	forked_from: Option<&str>,
) -> Vec<NativeNode> {
	let status = session_status_span(session.status);
	let created = session.created;
	let first = session.first_message.trim().to_string();
	let rest = if session
		.all_messages_text
		.starts_with(&session.first_message)
	{
		session.all_messages_text[session.first_message.len()..]
			.trim()
			.to_string()
	} else {
		String::new()
	};
	let mut conversation = Vec::new();
	if !first.is_empty() {
		let shown = if first.chars().count() > PREVIEW_EXCERPT_CHARS {
			format!("{}…", first.chars().take(PREVIEW_EXCERPT_CHARS).collect::<String>())
		} else {
			first
		};
		let mut props = Props::new();
		props.set_str("tone", "user");
		conversation.push(md(shown, props));
	}
	if !rest.is_empty() {
		let chars: Vec<char> = rest.chars().collect();
		let shown = if chars.len() > PREVIEW_EXCERPT_CHARS {
			format!(
				"…{}",
				chars[chars.len() - PREVIEW_EXCERPT_CHARS..]
					.iter()
					.collect::<String>()
			)
		} else {
			rest
		};
		conversation.push(md(shown, Props::new()));
	}
	let mut title_props = Props::new();
	title_props.set_str("text", session_label(session));
	title_props.set_role("omp.picker.title");
	let title = node(NodeKind::Text, title_props, Vec::new(), None);
	let pairs = vec![
		(
			"Folder".to_string(),
			if session.cwd.is_empty() {
				NodeText::Plain(String::new())
			} else {
				NodeText::Spans(cwd_spans(&session.cwd))
			},
		),
		(
			"Created".to_string(),
			created
				.map(|at| NodeText::plain(picker_date(at)))
				.unwrap_or_default(),
		),
		("Modified".to_string(), NodeText::plain(picker_date(session.modified))),
		("Size".to_string(), NodeText::plain(format_bytes(session.size as usize))),
		(
			"Status".to_string(),
			status
				.map(|span| NodeText::Spans(vec![span]))
				.unwrap_or_default(),
		),
		(
			"Forked from".to_string(),
			forked_from
				.map(|from| NodeText::plain(from.to_string()))
				.unwrap_or_default(),
		),
	];
	let mut children = vec![title];
	if let Some(facts) = kv(pairs) {
		children.push(facts);
	}
	if !conversation.is_empty() {
		let mut props = Props::new();
		props.set_str("head", "Conversation");
		children.push(node(NodeKind::Section, props, conversation, None));
	}
	children
}

/// Session search text: every searchable field joined.
#[must_use]
pub fn session_search_text(session: &SessionSelectorEntry) -> String {
	let parts = [
		session.id.as_str(),
		session.title.as_deref().unwrap_or(""),
		session.cwd.as_str(),
		session.first_message.as_str(),
		session.all_messages_text.as_str(),
		session.path.as_str(),
	];
	parts
		.iter()
		.filter(|part| !part.is_empty())
		.copied()
		.collect::<Vec<_>>()
		.join(" ")
}

/// Split a query into lowercase whitespace tokens.
#[must_use]
pub fn tokenize_session_query(query: &str) -> Vec<String> {
	query
		.trim()
		.to_lowercase()
		.split_whitespace()
		.map(str::to_string)
		.collect()
}

/// One ranked search hit; `index` is the session's position in the unfiltered
/// list (recency order).
#[derive(Clone, Debug, PartialEq)]
pub struct RankedSession<T> {
	/// The matched session.
	pub session: T,
	/// Fuzzy score; `0` for literal matches.
	pub score: f64,
	/// Position in the unfiltered list.
	pub index: usize,
}

/// Minimum per-token score below which a pure-fuzzy match is noise.
const MIN_PURE_FUZZY_TOKEN_SCORE: f64 = -20.0;

fn is_literal_match(text_lower: &str, tokens: &[String]) -> bool {
	tokens.iter().all(|token| text_lower.contains(token))
}

fn compare_recency<T: AsRef<SessionSelectorEntry> + AsMut<SessionSelectorEntry>>(a: &T, b: &T) -> std::cmp::Ordering {
	b.as_ref().modified.cmp(&a.as_ref().modified)
}

/// Score one non-literal session against every query token.
fn score_fuzzy_session<T: Clone + AsRef<SessionSelectorEntry> + AsMut<SessionSelectorEntry>>(
	session: &T,
	index: usize,
	tokens: &[String],
) -> Option<RankedSession<T>> {
	let text_lower = session_search_text(session.as_ref()).to_lowercase();
	let mut score = 0.0;
	let mut worst_token_score = f64::NEG_INFINITY;
	for token in tokens {
		let matched = fuzzy_match(token, &text_lower);
		if !matched.matches {
			return None;
		}
		score += matched.score;
		worst_token_score = worst_token_score.max(matched.score);
	}
	if worst_token_score >= MIN_PURE_FUZZY_TOKEN_SCORE {
		return None;
	}
	Some(RankedSession {
		session: session.clone(),
		score,
		index,
	})
}

/// Exact titles lead partial titles; other matches retain their existing
/// order.
fn prioritize_title_matches<T: Clone + AsRef<SessionSelectorEntry> + AsMut<SessionSelectorEntry>>(
	sessions: Vec<T>,
	tokens: &[String],
	literal: &[RankedSession<T>],
) -> Vec<T> {
	let query = tokens.join(" ");
	let mut exact: Vec<T> = Vec::new();
	let mut partial: Vec<T> = Vec::new();
	let mut title_matches: HashSet<String> = HashSet::new();
	for RankedSession { session, .. } in literal {
		let title = session
			.as_ref()
			.title
			.as_deref()
			.map(|title| collapse_whitespace(title).trim().to_lowercase())
			.unwrap_or_default();
		let path = session.as_ref().path.clone();
		if title == query {
			exact.push(session.clone());
		} else if !title.is_empty() && is_literal_match(&title, tokens) {
			partial.push(session.clone());
		} else {
			continue;
		}
		title_matches.insert(path);
	}
	if title_matches.is_empty() {
		return sessions;
	}
	exact.extend(partial);
	for session in sessions {
		if !title_matches.contains(&session.as_ref().path) {
			exact.push(session);
		}
	}
	exact
}

/// Filter and rank session picker search results. Exact and partial title
/// matches lead; other literal matches rank by recency; pure fuzzy matches
/// sort by score after them, weak fuzzy tokens are dropped as noise.
#[must_use]
pub fn rank_session_search_matches<T: Clone + AsRef<SessionSelectorEntry> + AsMut<SessionSelectorEntry>>(
	all_sessions: &[T],
	query: &str,
) -> Vec<T> {
	let tokens = tokenize_session_query(query);
	if tokens.is_empty() {
		return all_sessions.to_vec();
	}
	let mut literal: Vec<RankedSession<T>> = Vec::new();
	let mut fuzzy_matches: Vec<RankedSession<T>> = Vec::new();
	for (index, session) in all_sessions.iter().enumerate() {
		let text_lower = session_search_text(session.as_ref()).to_lowercase();
		if is_literal_match(&text_lower, &tokens) {
			literal.push(RankedSession {
				session: session.clone(),
				score: 0.0,
				index,
			});
			continue;
		}
		if let Some(matched) = score_fuzzy_session(session, index, &tokens) {
			fuzzy_matches.push(matched);
		}
	}
	literal.sort_by(|a, b| {
		compare_recency(&a.session, &b.session).then(a.index.cmp(&b.index))
	});
	fuzzy_matches.sort_by(|a, b| {
		a.score
			.total_cmp(&b.score)
			.then(compare_recency(&a.session, &b.session))
			.then(a.index.cmp(&b.index))
	});
	let mut out: Vec<T> = literal.iter().map(|m| m.session.clone()).collect();
	out.extend(fuzzy_matches.iter().map(|m| m.session.clone()));
	prioritize_title_matches(out, &tokens, &literal)
}

/// Combine metadata matches with prompt-history matches for ranking, using
/// both signals rather than replacing one with the other.
#[must_use]
pub fn merge_session_ranking<T: Clone + AsRef<SessionSelectorEntry> + AsMut<SessionSelectorEntry>>(
	all_sessions: &[T],
	fuzzy: Vec<T>,
	history_ids: &[String],
) -> Vec<T> {
	if history_ids.is_empty() {
		return fuzzy;
	}
	let mut by_id: HashMap<&str, &T> = HashMap::new();
	for session in all_sessions {
		by_id.entry(session.as_ref().id.as_str()).or_insert(session);
	}
	let mut history_matches: Vec<T> = Vec::new();
	let mut history_paths: HashSet<String> = HashSet::new();
	for id in history_ids {
		let Some(session) = by_id.get(id.as_str()) else {
			continue;
		};
		let path = session.as_ref().path.clone();
		if history_paths.contains(&path) {
			continue;
		}
		history_matches.push((*session).clone());
		history_paths.insert(path);
	}
	if history_matches.is_empty() {
		return fuzzy;
	}
	let mut out = history_matches;
	for session in fuzzy {
		if !history_paths.contains(&session.as_ref().path) {
			out.push(session);
		}
	}
	out
}

/// Delay before the prompt-history DB is consulted for the current query.
pub const HISTORY_MERGE_DEBOUNCE_MS: u64 = 150;
/// Minimum query length for history augmentation.
pub const HISTORY_MERGE_MIN_QUERY: usize = 2;

/// Scope tabs, in order.
pub const SCOPE_TABS: [(&str, &str); 2] = [
	("folder", "Current folder"),
	("all", "All projects"),
];

/// Session picker scope.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SessionSelectorScope {
	/// Sessions of the current folder.
	#[default]
	Folder,
	/// Sessions of all projects.
	All,
}

impl SessionSelectorScope {
	/// The wire id of the scope.
	#[must_use]
	pub fn as_str(self) -> &'static str {
		match self {
			Self::Folder => "folder",
			Self::All => "all",
		}
	}
}

/// Transient status line above the list: scope loading or an error.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionPickerMessage {
	/// What the message reports.
	pub kind: SessionPickerMessageKind,
	/// The message text.
	pub text: String,
}

/// Whether a [`SessionPickerMessage`] is loading or an error.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionPickerMessageKind {
	/// A scope is loading.
	Loading,
	/// A scan or action failed.
	Error,
}

/// An open delete confirmation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeleteChoice {
	/// Path of the session being deleted.
	pub session_path: String,
}

/// A row of the session list as the picker presents it.
#[derive(Clone, Debug, PartialEq)]
pub struct SessionPickerRow {
	/// Session path (row id).
	pub id: String,
	/// Row label.
	pub label: String,
	/// Detail text.
	pub detail: Option<String>,
	/// Fact cells.
	pub facts: Vec<(String, String)>,
	/// Status dot tone.
	pub dot: Option<String>,
	/// Whether the session is pinned.
	pub pinned: bool,
	/// Badges: `(text, tone)`.
	pub badges: Vec<(String, Option<String>)>,
}

/// The session list's picker data.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SessionPickerView {
	/// Every session in scope.
	pub items: Vec<SessionPickerRow>,
	/// Upserted rows (history badges).
	pub items_add: Vec<SessionPickerRow>,
	/// Query text.
	pub query: String,
	/// Search caret.
	pub cursor: usize,
	/// Row ids and group markers in display order.
	pub order: Vec<SessionOrderEntry>,
	/// Search-hit ranges by row id.
	pub hits: HashMap<String, Vec<(usize, usize)>>,
	/// Selected row id.
	pub selected: Option<String>,
	/// The live session's path.
	pub current: Vec<String>,
	/// Number of sessions in scope.
	pub total: usize,
}

/// One entry of [`SessionPickerView::order`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SessionOrderEntry {
	/// A group marker.
	Group {
		/// Group id.
		id: String,
		/// Group label.
		label: String,
		/// Rows in the group.
		count: usize,
	},
	/// A row id.
	Row(String),
}

/// Stable pinned-first partition over `sessions`.
fn pinned_first_with<T: Clone + AsRef<SessionSelectorEntry> + AsMut<SessionSelectorEntry>>(
	sessions: &[T],
	pinned_ids: &HashSet<String>,
) -> Vec<T> {
	let mut pinned: Vec<T> = Vec::new();
	let mut rest: Vec<T> = Vec::new();
	for session in sessions {
		if pinned_ids.contains(&session.as_ref().id) {
			pinned.push(session.clone());
		} else {
			rest.push(session.clone());
		}
	}
	if pinned.is_empty() {
		return rest;
	}
	pinned.extend(rest);
	pinned
}

/// Current wall-clock time in epoch milliseconds.
fn current_time_ms() -> i64 {
	std::time::SystemTime::now()
		.duration_since(std::time::UNIX_EPOCH)
		.map(|duration| duration.as_millis() as i64)
		.unwrap_or(0)
}

/// Custom session list component with multi-line items and search.
pub struct SessionList<T: Clone + AsRef<SessionSelectorEntry> + AsMut<SessionSelectorEntry>> {
	sessions: Vec<T>,
	composed: Vec<T>,
	show_cwd: bool,
	pinned_ids: HashSet<String>,
	current_path: Option<String>,
	search: Input,
	selected_index: usize,
	literal_ranked: Vec<RankedSession<T>>,
	fuzzy_ranked: Vec<RankedSession<T>>,
	history_ids: Vec<String>,
	history_matcher: Option<SessionHistoryMatcher>,
	last_query: String,
	had_filter_query: bool,
	selection_moved: bool,
	scroll_start_index: Option<usize>,
	terminal_rows: usize,
	hit_rows: Vec<Option<usize>>,
	pub on_select: Option<Box<dyn FnMut(&T)>>,
	pub on_cancel: Option<Box<dyn FnMut()>>,
	pub on_exit: Option<Box<dyn FnMut()>>,
	pub on_toggle_scope: Option<Box<dyn FnMut()>>,
	pub on_delete_request: Option<Box<dyn FnMut(&T)>>,
	pub on_rename_request: Option<Box<dyn FnMut(&T)>>,
	pub on_pin_request: Option<Box<dyn FnMut(&T, bool)>>,
}

impl<T: Clone + AsRef<SessionSelectorEntry> + AsMut<SessionSelectorEntry>> SessionList<T> {
	/// Build the list over `sessions`, selecting the live session when it is
	/// visible.
	pub fn new(
		sessions: Vec<T>,
		show_cwd: bool,
		history_matcher: Option<SessionHistoryMatcher>,
		terminal_rows: usize,
		pinned_ids: HashSet<String>,
		current_session_path: Option<String>,
	) -> Self {
		let composed = pinned_first_with(&sessions, &pinned_ids);
		let mut list = SessionList {
			composed,
			sessions,
			show_cwd,
			pinned_ids,
			current_path: current_session_path,
			search: Input::new(),
			selected_index: 0,
			literal_ranked: Vec::new(),
			fuzzy_ranked: Vec::new(),
			history_ids: Vec::new(),
			history_matcher,
			last_query: String::new(),
			had_filter_query: false,
			selection_moved: false,
			scroll_start_index: None,
			terminal_rows: terminal_rows.max(8),
			hit_rows: Vec::new(),
			on_select: None,
			on_cancel: None,
			on_exit: None,
			on_toggle_scope: None,
			on_delete_request: None,
			on_rename_request: None,
			on_pin_request: None,
		};
		list.select_current_session();
		list
	}

	/// The live query.
	pub fn query(&self) -> &str {
		self.search.value()
	}

	/// The query input, for hosts that draw it.
	pub fn search_input(&self) -> &Input {
		&self.search
	}

	/// Focus the live session when it is in the visible list.
	fn select_current_session(&mut self) {
		let Some(current) = self.current_path.as_deref() else {
			return;
		};
		if let Some(index) = self
			.visible_items()
			.iter()
			.position(|session| session.as_ref().path == current)
		{
			self.selected_index = index;
		}
	}

	/// All sessions, selection order (pinned first).
	#[must_use]
	pub fn sessions(&self) -> &[T] {
		&self.sessions
	}

	/// Whether the cwd column is on.
	#[must_use]
	pub fn show_cwd(&self) -> bool {
		self.show_cwd
	}

	/// Selected index into [`Self::visible_items`].
	#[must_use]
	pub fn selected_index(&self) -> usize {
		self.selected_index
	}

	/// The selected session.
	#[must_use]
	pub fn selected_session(&self) -> Option<&T> {
		self.visible_items().get(self.selected_index)
	}

	/// Whether the selected session is pinned.
	#[must_use]
	pub fn selected_pinned(&self) -> bool {
		self.selected_session()
			.is_some_and(|session| self.pinned_ids.contains(&session.as_ref().id))
	}

	/// Replace the visible dataset keeping the query and selection.
	pub fn refresh_sessions(&mut self, sessions: Vec<T>, show_cwd: bool) {
		let selected = self
			.selected_session()
			.map(|session| session.as_ref().path.clone());
		let moved = self.selection_moved;
		self.sessions = sessions;
		self.show_cwd = show_cwd;
		let query = self.search.value().to_string();
		self.filter_sessions(&query);
		if let Some(selected) = selected {
			self.set_selected_key(&selected);
		}
		self.selection_moved = moved;
	}

	/// Replace the visible dataset, e.g. when toggling scope.
	pub fn set_sessions(&mut self, sessions: Vec<T>, show_cwd: bool, pinned_ids: Option<HashSet<String>>) {
		self.sessions = sessions;
		self.show_cwd = show_cwd;
		if let Some(pinned) = pinned_ids {
			self.pinned_ids = pinned;
		}
		self.selected_index = 0;
		self.scroll_start_index = None;
		let query = self.search.value().to_string();
		self.filter_sessions(&query);
		self.select_current_session();
	}

	/// Stable partition: pinned sessions first, each group in incoming order.
	fn pinned_first(&self, sessions: Vec<T>) -> Vec<T> {
		if self.pinned_ids.is_empty() {
			return sessions;
		}
		let (pinned, rest): (Vec<T>, Vec<T>) = sessions
			.into_iter()
			.partition(|session| self.pinned_ids.contains(&session.as_ref().id));
		if pinned.is_empty() {
			return rest;
		}
		let mut out = pinned;
		out.extend(rest);
		out
	}

	/// The visible (ranked and partitioned) sessions.
	#[must_use]
	pub fn visible_items(&self) -> &[T] {
		&self.composed
	}

	/// Re-rank the listing for `query`, keeping the selection.
	pub fn filter_sessions(&mut self, query: &str) {
		self.selection_moved = false;
		self.history_ids.clear();
		self.literal_ranked.clear();
		self.fuzzy_ranked.clear();
		let tokens = tokenize_session_query(query);
		let had_query = self.had_filter_query;
		let query_changed = query != self.last_query;
		self.had_filter_query = !tokens.is_empty();
		self.last_query = query.to_string();
		if query_changed {
			self.scroll_start_index = None;
		}
		if tokens.is_empty() {
			let items = self.pinned_first(self.sessions.clone());
			self.selected_index = self
				.selected_index
				.min(items.len().saturating_sub(1));
			self.composed = items;
			if had_query {
				self.select_current_session();
			}
			self.schedule_history_merge(query);
			return;
		}
		let mut literal: Vec<RankedSession<T>> = Vec::new();
		for (index, session) in self.sessions.iter().enumerate() {
			let text_lower = session_search_text(session.as_ref()).to_lowercase();
			if is_literal_match(&text_lower, &tokens) {
				literal.push(RankedSession {
					session: session.clone(),
					score: 0.0,
					index,
				});
			}
		}
		literal.sort_by(|a, b| {
			compare_recency(&a.session, &b.session).then(a.index.cmp(&b.index))
		});
		self.literal_ranked = literal;
		for (index, session) in self.sessions.iter().enumerate() {
			let text_lower = session_search_text(session.as_ref()).to_lowercase();
			if is_literal_match(&text_lower, &tokens) {
				continue;
			}
			if let Some(matched) = score_fuzzy_session(session, index, &tokens) {
				self.fuzzy_ranked.push(matched);
			}
		}
		self.compose_filtered();
		if query_changed {
			self.selected_index = 0;
		}
		self.schedule_history_merge(query);
	}
}

// The composed visible list lives beside the ranking inputs; split out of the
// struct literal above for readability, then kept in sync here.
impl<T: Clone + AsRef<SessionSelectorEntry> + AsMut<SessionSelectorEntry>> SessionList<T> {
	/// The composed visible list of the current query.
	#[must_use]
	pub fn composed(&self) -> &[T] {
		&self.composed
	}

	fn compose_filtered(&mut self) {
		self.fuzzy_ranked.sort_by(|a, b| {
			a.score
				.total_cmp(&b.score)
				.then(compare_recency(&a.session, &b.session))
				.then(a.index.cmp(&b.index))
		});
		let mut base: Vec<T> = self
			.literal_ranked
			.iter()
			.map(|m| m.session.clone())
			.collect();
		base.extend(self.fuzzy_ranked.iter().map(|m| m.session.clone()));
		let tokens = tokenize_session_query(self.search.value());
		let merged = if self.history_ids.is_empty() {
			base
		} else {
			merge_session_ranking(&self.sessions, base, &self.history_ids)
		};
		let composed = prioritize_title_matches(merged, &tokens, &self.literal_ranked);
		let composed = self.pinned_first(composed);
		if self.selection_moved {
			self.composed = composed;
			return;
		}
		self.selected_index = self
			.selected_index
			.min(composed.len().saturating_sub(1));
		self.composed = composed;
	}

	/// Query the history matcher (hosts debounce it themselves).
	fn schedule_history_merge(&mut self, query: &str) {
		let trimmed = query.trim();
		if trimmed.len() < HISTORY_MERGE_MIN_QUERY {
			return;
		}
		let Some(matcher) = self.history_matcher.as_mut() else {
			return;
		};
		let ids = matcher(trimmed);
		if ids.is_empty() {
			return;
		}
		self.history_ids = ids;
		self.compose_filtered();
	}

	/// Select the visible row at `index`.
	pub fn set_selected_index(&mut self, index: usize) {
		if !self.composed.is_empty() {
			self.selected_index = index.min(self.composed.len() - 1);
		}
	}

	/// Select the visible row whose path equals `key`.
	pub fn set_selected_key(&mut self, key: &str) -> bool {
		let Some(index) = self
			.composed
			.iter()
			.position(|session| session.as_ref().path == key)
		else {
			return false;
		};
		self.selected_index = index;
		true
	}

	/// Move the selection, clamped.
	pub fn move_selection(&mut self, delta: isize) {
		if self.composed.is_empty() {
			return;
		}
		let next = self.selected_index as isize + delta;
		self.selected_index = next.clamp(0, self.composed.len() as isize - 1) as usize;
	}

	/// Remove a session by path.
	pub fn remove_session(&mut self, path: &str) {
		let Some(index) = self
			.sessions
			.iter()
			.position(|session| session.as_ref().path == path)
		else {
			return;
		};
		self.sessions.remove(index);
		let query = self.search.value().to_string();
		self.filter_sessions(&query);
		if self.selected_index >= self.composed.len() {
			self.selected_index = self.composed.len().saturating_sub(1);
		}
	}

	/// Change a session's title in place, keeping the selection.
	pub fn rename_session_title(&mut self, path: &str, title: &str) {
		let Some(entry) = self
			.sessions
			.iter_mut()
			.find(|session| session.as_ref().path == path)
		else {
			return;
		};
		entry.as_mut().title = Some(title.to_string());
		let query = self.search.value().to_string();
		self.filter_sessions(&query);
	}

	/// Apply a persisted pin change; the list re-partitions and the cursor
	/// follows the session.
	pub fn set_pinned(&mut self, session_id: &str, pinned: bool) {
		if pinned {
			self.pinned_ids.insert(session_id.to_string());
		} else {
			self.pinned_ids.remove(session_id);
		}
		self.selection_moved = true;
		if self.had_filter_query {
			self.compose_filtered();
		} else {
			let sessions = self.sessions.clone();
			self.composed = self.pinned_first(sessions);
		}
	}

	/// Mouse-click outcome: select and resume the session at `index`.
	pub fn select_and_confirm(&mut self, index: usize) {
		let Some(session) = self.composed.get(index).cloned() else {
			return;
		};
		self.selected_index = index;
		if let Some(on_select) = self.on_select.as_mut() {
			on_select(&session);
		}
	}

	/// Native click on a session item: select and resume it.
	pub fn confirm_session(&mut self, path: &str) {
		if let Some(index) = self
			.composed
			.iter()
			.position(|session| session.as_ref().path == path)
		{
			self.select_and_confirm(index);
		}
	}

	/// Native row click: move the selection there.
	pub fn select_session(&mut self, path: &str) {
		if self.set_selected_key(path) {
			self.selection_moved = true;
		}
	}

	/// Enter: resume the selected session.
	pub fn resume_selected(&mut self) {
		let Some(session) = self.composed.get(self.selected_index).cloned() else {
			return;
		};
		if let Some(on_select) = self.on_select.as_mut() {
			on_select(&session);
		}
	}

	/// Ask the parent to confirm deleting the selected session.
	pub fn request_delete(&mut self) {
		let Some(session) = self.composed.get(self.selected_index).cloned() else {
			return;
		};
		if let Some(on_request) = self.on_delete_request.as_mut() {
			on_request(&session);
		}
	}

	/// Ask the parent to rename the selected session.
	pub fn request_rename(&mut self) {
		let Some(session) = self.composed.get(self.selected_index).cloned() else {
			return;
		};
		if let Some(on_request) = self.on_rename_request.as_mut() {
			on_request(&session);
		}
	}

	/// Ask the parent to pin or unpin the selected session (default: flip it).
	pub fn request_pin(&mut self, pinned: Option<bool>) {
		let Some(session) = self.composed.get(self.selected_index).cloned() else {
			return;
		};
		let is_pinned = self.pinned_ids.contains(&session.as_ref().id);
		if pinned.unwrap_or(!is_pinned) != is_pinned {
			if let Some(on_request) = self.on_pin_request.as_mut() {
				on_request(&session, !is_pinned);
			}
		}
	}

	/// Empty the search query.
	pub fn clear_search(&mut self) {
		self.search.set_value("");
		self.filter_sessions("");
	}

	/// Label of the session at `path` in this listing, else its file name.
	#[must_use]
	pub fn parent_label(&self, path: &str) -> String {
		if let Some(parent) = self
			.sessions
			.iter()
			.find(|session| session.as_ref().path == path)
		{
			return session_label(parent.as_ref());
		}
		Path::new(path)
			.file_stem()
			.map(|stem| stem.to_string_lossy().into_owned())
			.unwrap_or_else(|| path.to_string())
	}

	/// Line budget for the session rows in one frame.
	#[must_use]
	pub fn line_budget(&self) -> usize {
		const CHROME: usize = 7;
		const RESERVE: usize = 1;
		self.terminal_rows.saturating_sub(CHROME + RESERVE).max(8)
	}

	fn page_size(&self) -> isize {
		(self.line_budget() / 4).max(2) as isize
	}

	/// The picker view: rows, ordering and search hits.
	#[must_use]
	pub fn picker_view(&self, now: i64) -> SessionPickerView {
		let query = self.search.value().to_string();
		let mut items = Vec::with_capacity(self.sessions.len());
		for session in &self.sessions {
			items.push(self.picker_row(session, now));
		}
		let tokens = tokenize_session_query(&query);
		let mut order = Vec::new();
		let mut hits = HashMap::new();
		if tokens.is_empty() {
			let start_of_today = now - now.rem_euclid(DAY_MS);
			let groups: Vec<SessionDayGroup> = self
				.composed
				.iter()
				.map(|session| {
					if self.pinned_ids.contains(&session.as_ref().id) {
						SessionDayGroup {
							id: PINNED_GROUP.0,
							label: PINNED_GROUP.1,
						}
					} else {
						session_day_group(session.as_ref().modified, start_of_today)
					}
				})
				.collect();
			let mut index = 0;
			while index < self.composed.len() {
				let group = &groups[index];
				let mut count = 1;
				while index + count < self.composed.len() && groups[index + count].id == group.id {
					count += 1;
				}
				order.push(SessionOrderEntry::Group {
					id: format!("{}-{index}", group.id),
					label: group.label.to_string(),
					count,
				});
				for offset in 0..count {
					order.push(SessionOrderEntry::Row(
						self.composed[index + offset].as_ref().path.clone(),
					));
				}
				index += count;
			}
		} else {
			let labels: HashMap<&str, &str> = items
				.iter()
				.map(|row| (row.id.as_str(), row.label.as_str()))
				.collect();
			for session in &self.composed {
				let path = &session.as_ref().path;
				order.push(SessionOrderEntry::Row(path.clone()));
				let Some(label) = labels.get(path.as_str()) else {
					continue;
				};
				let token_refs: Vec<&str> = tokens.iter().map(String::as_str).collect();
				let ranges = picker_hits(label, &token_refs);
				if !ranges.is_empty() {
					hits.insert(path.clone(), ranges);
				}
			}
		}
		let selected = self
			.selected_session()
			.map(|session| session.as_ref().path.clone());
		SessionPickerView {
			items,
			items_add: Vec::new(),
			query,
			cursor: self.search.value().encode_utf16().count(),
			order,
			hits,
			selected,
			current: self.current_path.iter().cloned().collect(),
			total: self.sessions.len(),
		}
	}

	fn picker_row(&self, session: &T, now: i64) -> SessionPickerRow {
		let session = session.as_ref();
		let message = collapse_whitespace(&session.first_message).trim().to_string();
		let title = session
			.title
			.as_deref()
			.map(|title| collapse_whitespace(title).trim().to_string())
			.unwrap_or_default();
		let mut badges = Vec::new();
		if let Some(parent) = &session.parent_session_path {
			badges.push((
				"fork".to_string(),
				None,
			));
			let _ = parent;
		}
		if Some(session.path.as_str()) == self.current_path.as_deref() {
			badges.push(("current".to_string(), Some("accent".to_string())));
		}
		let detail = if self.show_cwd {
			if session.cwd.is_empty() {
				None
			} else {
				Some(shorten_path(&session.cwd))
			}
		} else if !title.is_empty() {
			Some(message.clone())
		} else {
			None
		};
		SessionPickerRow {
			id: session.path.clone(),
			label: if !title.is_empty() {
				title
			} else if !message.is_empty() {
				message
			} else {
				session.id.clone()
			},
			detail,
			facts: vec![
				("when".to_string(), picker_age(session.modified, now)),
				("size".to_string(), format_bytes(session.size as usize)),
			],
			dot: session_status_tone(session.status).map(str::to_string),
			pinned: self.pinned_ids.contains(&session.id),
			badges,
		}
	}

	/// The list as a native `list` node; items are keyed by session path.
	#[must_use]
	pub fn describe_list(&self, theme: &Theme, now: i64) -> NativeNode {
		let current_path = self.current_path.as_deref();
		let items: Vec<NativeNode> = self
			.composed
			.iter()
			.map(|session| describe_session(session.as_ref(), current_path, &self.pinned_ids, self.show_cwd, theme, now))
			.collect();
		let empty_text = if self.show_cwd {
			vec![span("No sessions found", Some("muted"))]
		} else {
			vec![
				span("No sessions in current folder. Press ", Some("muted")),
				span("tab", Some("key")),
				span(" to view all.", Some("muted")),
			]
		};
		let mut props = Props::new();
		let selected = self
			.selected_session()
			.map(|session| session.as_ref().path.clone());
		match selected {
			Some(selected) => props.set_str("selected", selected),
			None => {
				props.insert("selected", serde_json::Value::Null);
			}
		}
		let query = self.search.value().trim();
		if !query.is_empty() {
			props.set_str("filter", query);
		}
		props.insert(
			"empty",
			serde_json::Value::Array(empty_text.iter().map(|span| span.to_value()).collect()),
		);
		props.set_bool("virtual", true);
		node(NodeKind::List, props, items, Some("list"))
	}
}

/// One session row node in the native list.
fn describe_session(
	session: &SessionSelectorEntry,
	current_path: Option<&str>,
	pinned_ids: &HashSet<String>,
	show_cwd: bool,
	theme: &Theme,
	now: i64,
) -> NativeNode {
	let date = format_session_date(session.modified, now);
	let pinned = pinned_ids.contains(&session.id);
	let current = current_path.is_some_and(|path| path == session.path);
	let message = collapse_whitespace(&session.first_message).trim().to_string();
	let mut detail: Vec<crate::native::node::Span> = Vec::new();
	let mut push = |part: crate::native::node::Span| {
		if !detail.is_empty() {
			detail.push(span(
				format!(" {} ", theme.sep().dot.as_str()),
				Some("dim"),
			));
		}
		detail.push(part);
	};
	if session.title.is_some() && !message.is_empty() {
		push(span(message.clone(), Some("dim")));
	}
	if current {
		push(span("current", Some("accent")));
	}
	if let Some(status) = session_status_span(session.status) {
		push(status);
	}
	if session.parent_session_path.is_some() {
		push(span("fork", Some("dim")));
	}
	if show_cwd && !session.cwd.is_empty() {
		push(span(shorten_path(&session.cwd), Some("path")));
	}
	let label = session
		.title
		.as_deref()
		.map(collapse_whitespace)
		.filter(|title| !title.trim().is_empty())
		.unwrap_or_else(|| {
			if !message.is_empty() {
				message
			} else {
				session.id.clone()
			}
		});
	let mut props = Props::new();
	props.set_str("label", label);
	if !detail.is_empty() {
		props.insert("detail", crate::native::node::Text::from(detail).to_value());
	}
	props.insert(
		"value",
		crate::native::node::Text::from(vec![span(
			format!("{date} {} {}", theme.sep().dot.as_str(), format_bytes(session.size as usize)),
			Some("dim"),
		)])
		.to_value(),
	);
	if pinned {
		props.set_str("icon", "pin");
	}
	crate::native::describe::item(session.path.clone(), props)
}

impl<T: Clone + AsRef<SessionSelectorEntry> + AsMut<SessionSelectorEntry>> Component for SessionList<T> {
	fn render(&mut self, width: u16) -> Vec<String> {
		let mut lines: Vec<String> = self.search.render(width);
		lines.push(String::new());
		self.hit_rows.clear();
		if self.composed.is_empty() {
			let hint = if self.show_cwd {
				"No sessions found".to_string()
			} else {
				"No sessions in current folder. Press tab to view all.".to_string()
			};
			lines.push(truncate_to_width(&hint, width as usize, Ellipsis::Unicode));
			return lines;
		}
		let budget = self.line_budget();
		let heights: Vec<usize> = self
			.composed
			.iter()
			.map(|session| if session.as_ref().title.is_some() { 4 } else { 3 })
			.collect();
		let options = MenuWindowOptions {
			alignment: if self.scroll_start_index.is_some() {
				MenuAlignment::Nearest
			} else {
				MenuAlignment::Center
			},
			current_start_index: self.scroll_start_index,
		};
		let window = get_menu_window(&heights, self.selected_index, budget, Some(&options));
		self.scroll_start_index = Some(window.start_index);
		let row_width = content_row_width(width as usize, window.total_rows, budget);
		let mut session_lines: Vec<String> = Vec::new();
		let mut session_row_index: Vec<usize> = Vec::new();
		for index in window.start_index..window.end_index {
			let block_start = session_lines.len();
			let Some(session) = self.composed.get(index) else {
				continue;
			};
			let entry = session.as_ref();
			let is_selected = index == self.selected_index;
			let normalized = collapse_whitespace(&entry.first_message);
			let cursor_symbol = "› ";
			let cursor_width = visible_width(cursor_symbol);
			let cursor = if is_selected {
				cursor_symbol.to_string()
			} else {
				padding(cursor_width)
			};
			let max_width = row_width.saturating_sub(cursor_width);
			let pinned = self.pinned_ids.contains(&entry.id);
			let pin_prefix = if pinned { "📌 " } else { "" };
			let pin_width = if pinned { visible_width(pin_prefix) } else { 0 };
			let max_text_width = max_width.saturating_sub(pin_width);
			if let Some(title) = &entry.title {
				let truncated = truncate_to_width(title, max_text_width, Ellipsis::Unicode);
				session_lines.push(format!("{cursor}{pin_prefix}{truncated}"));
				let preview = truncate_to_width(normalized.trim(), max_width, Ellipsis::Unicode);
				session_lines.push(format!("  {preview}"));
			} else {
				let truncated = truncate_to_width(normalized.trim(), max_text_width, Ellipsis::Unicode);
				session_lines.push(format!("{cursor}{pin_prefix}{truncated}"));
			}
			let mut metadata = format!(
				"  {} · {}",
				format_session_date(entry.modified, current_time_ms()),
				format_bytes(entry.size as usize)
			);
			if Some(entry.path.as_str()) == self.current_path.as_deref() {
				metadata.push_str(" · current");
			}
			if let Some(status) = entry.status {
				metadata.push_str(&format!(" · {}", status.as_str()));
			}
			if entry.parent_session_path.is_some() {
				metadata.push_str(" · fork");
			}
			if self.show_cwd && !entry.cwd.is_empty() {
				metadata.push_str(&format!(" · {}", shorten_path(&entry.cwd)));
			}
			session_lines.push(truncate_to_width(&metadata, row_width, Ellipsis::Unicode));
			if index < window.end_index - 1 {
				session_lines.push(String::new());
			}
			for row_index in block_start..session_lines.len() {
				if row_index < session_row_index.len() {
					session_row_index[row_index] = index;
				} else {
					session_row_index.push(index);
				}
			}
		}
		let region_start = lines.len();
		for (offset, line) in session_lines.iter().enumerate() {
			self.hit_rows.push(session_row_index.get(offset).copied());
			lines.push(pad_to_width(line, width as usize));
		}
		let _ = region_start;
		lines
	}

	fn handle_input(&mut self, key: &Key) {
		match key {
			Key::Up => {
				self.selection_moved = true;
				self.move_selection(-1);
				return;
			}
			Key::Down => {
				self.selection_moved = true;
				self.move_selection(1);
				return;
			}
			Key::PageUp => {
				self.selection_moved = true;
				let page = self.page_size();
				self.move_selection(-page);
				return;
			}
			Key::PageDown => {
				self.selection_moved = true;
				let page = self.page_size();
				self.move_selection(page);
				return;
			}
			Key::Enter => {
				self.resume_selected();
				return;
			}
			Key::Esc => {
				if let Some(on_cancel) = self.on_cancel.as_mut() {
					on_cancel();
				}
				return;
			}
			Key::Ctrl('c') => {
				if let Some(on_exit) = self.on_exit.as_mut() {
					on_exit();
				}
				return;
			}
			Key::Tab => {
				if let Some(toggle) = self.on_toggle_scope.as_mut() {
					toggle();
				}
				return;
			}
			Key::F(2) => {
				self.request_rename();
				return;
			}
			Key::Delete | Key::Backspace if self.search.value().is_empty() => {
				self.request_delete();
				return;
			}
			_ => {}
		}
		self.search.handle_input(key);
		let query = self.search.value().to_string();
		self.filter_sessions(&query);
	}

	fn handle_mouse(&mut self, event: &crate::mouse::MouseEvent) {
		match event.kind {
			crate::mouse::MouseKind::ScrollUp => self.move_selection(-1),
			crate::mouse::MouseKind::ScrollDown => self.move_selection(1),
			_ => {}
		}
	}
}

/// Options of the session selector.
pub struct SessionSelectorOptions<T: Clone + AsRef<SessionSelectorEntry> + AsMut<SessionSelectorEntry>> {
	/// Called with the chosen session.
	pub on_delete: Option<Box<dyn FnMut(&T) -> Result<bool, String>>>,
	/// Rename the selected session; `Ok(Some(title))` carries the stored title.
	pub on_rename: Option<Box<dyn FnMut(&T, &str) -> Result<Option<String>, String>>>,
	/// Persist a pin change.
	pub on_set_pinned: Option<Box<dyn FnMut(&T, bool) -> Result<(), String>>>,
	/// Prompt-history matcher for the current query.
	pub history_matcher: Option<SessionHistoryMatcher>,
	/// Loads sessions across all projects for the all-projects scope.
	pub load_all_sessions: Option<Box<dyn FnMut() -> Result<Vec<T>, String>>>,
	/// Preloaded all-projects listing.
	pub all_sessions: Option<Vec<T>>,
	/// Picker heading; defaults to `Resume Session`.
	pub title: Option<String>,
	/// Primary action label; defaults to `Resume`.
	pub action_label: Option<String>,
	/// Fixed scope label, or `Some(None)` to omit the scope suffix.
	pub scope_label: Option<Option<String>>,
	/// Show each session's working directory in the list.
	pub show_cwd: Option<bool>,
	/// The standalone `--resume` picker fills the screen surface.
	pub standalone: bool,
	/// Set of pinned session ids.
	pub pinned_ids: HashSet<String>,
	/// Path of the live session.
	pub current_session_path: Option<String>,
	/// Reads the live terminal height.
	pub terminal_rows: usize,
}

impl<T: Clone + AsRef<SessionSelectorEntry> + AsMut<SessionSelectorEntry>> Default for SessionSelectorOptions<T> {
	fn default() -> Self {
		SessionSelectorOptions {
			on_delete: None,
			on_rename: None,
			on_set_pinned: None,
			history_matcher: None,
			load_all_sessions: None,
			all_sessions: None,
			title: None,
			action_label: None,
			scope_label: None,
			show_cwd: None,
			standalone: false,
			pinned_ids: HashSet::new(),
			current_session_path: None,
			terminal_rows: 24,
		}
	}
}

/// The rename editor: an input over the old title.
struct RenameState {
	session_path: String,
	input: Input,
}

/// Component that renders a session selector with an optional confirmation
/// dialog.
pub struct SessionSelectorComponent<T: Clone + AsRef<SessionSelectorEntry> + AsMut<SessionSelectorEntry>> {
	list: SessionList<T>,
	on_select: Box<dyn FnMut(&T)>,
	on_cancel: Box<dyn FnMut()>,
	on_exit: Box<dyn FnMut()>,
	on_delete: Option<Box<dyn FnMut(&T) -> Result<bool, String>>>,
	on_rename: Option<Box<dyn FnMut(&T, &str) -> Result<Option<String>, String>>>,
	on_set_pinned: Option<Box<dyn FnMut(&T, bool) -> Result<(), String>>>,
	load_all_sessions: Option<Box<dyn FnMut() -> Result<Vec<T>, String>>>,
	on_request_render: Option<Box<dyn FnMut()>>,
	folder_sessions: Vec<T>,
	global_sessions: Option<Vec<T>>,
	show_cwd_option: Option<bool>,
	scope: SessionSelectorScope,
	scope_label: Option<Option<String>>,
	title: String,
	action_label: String,
	standalone: bool,
	deleted_paths: HashSet<String>,
	renamed_titles: HashMap<String, String>,
	closed: bool,
	input_locked: bool,
	message: Option<SessionPickerMessage>,
	delete_choice: Option<DeleteChoice>,
	rename: Option<RenameState>,
	toggling: bool,
	theme: Theme,
}

impl<T: Clone + AsRef<SessionSelectorEntry> + AsMut<SessionSelectorEntry>> SessionSelectorComponent<T> {
	/// Build the selector over `sessions`.
	pub fn new(
		sessions: Vec<T>,
		on_select: impl FnMut(&T) + 'static,
		on_cancel: impl FnMut() + 'static,
		on_exit: impl FnMut() + 'static,
		options: SessionSelectorOptions<T>,
		theme: Theme,
	) -> Self {
		let title = options.title.clone().unwrap_or_else(|| "Resume Session".to_string());
		let action_label = options.action_label.clone().unwrap_or_else(|| "Resume".to_string());
		let show_cwd_option = options.show_cwd;
		let folder_show_cwd = show_cwd_option.unwrap_or_else(|| {
			let dirs: HashSet<&Path> = sessions
				.iter()
				.map(|session| Path::new(&session.as_ref().path).parent().unwrap_or(Path::new("")))
				.collect();
			dirs.len() > 1
		});
		let list = SessionList::new(
			sessions.clone(),
			folder_show_cwd,
			options.history_matcher,
			options.terminal_rows,
			options.pinned_ids,
			options.current_session_path,
		);

		let global_sessions = options.all_sessions;
		SessionSelectorComponent {
			list,
			on_select: Box::new(on_select),
			on_cancel: Box::new(on_cancel),
			on_exit: Box::new(on_exit),
			on_delete: options.on_delete,
			on_rename: options.on_rename,
			on_set_pinned: options.on_set_pinned,
			load_all_sessions: options.load_all_sessions,
			on_request_render: None,
			folder_sessions: sessions,
			global_sessions,
			show_cwd_option,
			scope: SessionSelectorScope::Folder,
			scope_label: options.scope_label,
			title,
			action_label,
			standalone: options.standalone,
			deleted_paths: HashSet::new(),
			renamed_titles: HashMap::new(),
			closed: false,
			input_locked: false,
			message: None,
			delete_choice: None,
			rename: None,
			toggling: false,
			theme,
		}
	}

	/// The semantic role of the ANSI sheet.
	#[must_use]
	pub fn native_role(&self) -> &'static str {
		"omp.overlay.sessions"
	}

	/// The shown scope.
	#[must_use]
	pub fn scope(&self) -> SessionSelectorScope {
		self.scope
	}

	/// The current heading.
	#[must_use]
	pub fn header_label(&self) -> String {
		if self.scope_label == Some(None) {
			return self.title.clone();
		}
		let label = self
			.scope_label
			.clone()
			.flatten()
			.unwrap_or_else(|| match self.scope {
				SessionSelectorScope::All => "all projects".to_string(),
				SessionSelectorScope::Folder => "current folder".to_string(),
			});
		format!("{} ({label})", self.title)
	}

	/// The session list, for hosts and tests.
	#[must_use]
	pub fn session_list(&self) -> &SessionList<T> {
		&self.list
	}

	/// The session list, mutably.
	pub fn session_list_mut(&mut self) -> &mut SessionList<T> {
		&mut self.list
	}

	/// Whether the picker can toggle scope.
	#[must_use]
	pub fn can_toggle_scope(&self) -> bool {
		self.load_all_sessions.is_some() || self.global_sessions.is_some()
	}

	/// Toggle between current-folder and all-projects scope. The global listing
	/// is loaded lazily on first switch and cached.
	pub fn toggle_scope(&mut self) -> Result<(), String> {
		if self.toggling || self.delete_choice.is_some() || self.rename.is_some() {
			return Ok(());
		}
		match self.scope {
			SessionSelectorScope::Folder => {
				if self.global_sessions.is_none() {
					let Some(load) = self.load_all_sessions.as_mut() else {
						return Ok(());
					};
					self.toggling = true;
					self.message = Some(SessionPickerMessage {
						kind: SessionPickerMessageKind::Loading,
						text: "Loading all projects…".to_string(),
					});
					let result = load();
					self.toggling = false;
					match result {
						Ok(sessions) => {
							let sessions = self.with_edits(sessions);
							self.global_sessions = Some(sessions.clone());
							self.list.set_sessions(sessions, true, None);
						}
						Err(error) => {
							self.show_error(&error);
							return Err(error);
						}
					}
					self.message = None;
				} else if let Some(global) = self.global_sessions.clone() {
					self.list.set_sessions(global, true, None);
				}
				self.scope = SessionSelectorScope::All;
			}
			SessionSelectorScope::All => {
				let folder_show_cwd = self.folder_show_cwd();
				self.list
					.set_sessions(self.folder_sessions.clone(), folder_show_cwd, None);
				self.scope = SessionSelectorScope::Folder;
			}
		}
		Ok(())
	}

	fn folder_show_cwd(&self) -> bool {
		self.show_cwd_option.unwrap_or_else(|| {
			let dirs: HashSet<&Path> = self
				.folder_sessions
				.iter()
				.map(|session| Path::new(&session.as_ref().path).parent().unwrap_or(Path::new("")))
				.collect();
			dirs.len() > 1
		})
	}

	/// `sessions` with this picker's deletes and renames applied.
	fn with_edits(&self, sessions: Vec<T>) -> Vec<T> {
		if self.deleted_paths.is_empty() && self.renamed_titles.is_empty() {
			return sessions;
		}
		let mut kept: Vec<T> = sessions
			.into_iter()
			.filter(|session| !self.deleted_paths.contains(&session.as_ref().path))
			.collect();
		for session in &mut kept {
			if let Some(title) = self.renamed_titles.get(&session.as_ref().path) {
				session.as_mut().title = Some(title.clone());
			}
		}
		kept
	}

	/// Install the render-request hook used after async work.
	pub fn set_on_request_render(&mut self, callback: impl FnMut() + 'static) {
		self.on_request_render = Some(Box::new(callback));
	}

	fn request_render(&mut self) {
		if let Some(callback) = self.on_request_render.as_mut() {
			callback();
		}
	}

	/// Ignore input after selection while the host resumes the session.
	pub fn lock_input(&mut self) {
		self.input_locked = true;
	}

	/// Re-enable input after a failed resume.
	pub fn unlock_input(&mut self) {
		self.input_locked = false;
		self.closed = false;
	}

	/// Whether the picker resolved or was disposed.
	#[must_use]
	pub fn is_closed(&self) -> bool {
		self.closed
	}

	/// Close the picker through `on_cancel`.
	pub fn close(&mut self) {
		self.closed = true;
		(self.on_cancel)();
	}

	fn show_error(&mut self, message: &str) {
		self.message = Some(SessionPickerMessage {
			kind: SessionPickerMessageKind::Error,
			text: format!("Error: {}", collapse_whitespace(message)),
		});
	}

	fn clear_error(&mut self) {
		self.message = None;
	}

	/// Ask to delete `session`: open the confirmation strip.
	fn show_delete_confirmation(&mut self, session: &T) {
		self.delete_choice = Some(DeleteChoice {
			session_path: session.as_ref().path.clone(),
		});
		self.request_render();
	}

	/// Run the confirmed delete.
	fn confirm_delete(&mut self) {
		let Some(choice) = self.delete_choice.take() else {
			return;
		};
		let session = self
			.folder_sessions
			.iter()
			.chain(self.global_sessions.iter().flatten())
			.find(|session| session.as_ref().path == choice.session_path)
			.cloned();
		let Some(session) = session else {
			return;
		};
		self.clear_error();
		let Some(on_delete) = self.on_delete.as_mut() else {
			return;
		};
		match on_delete(&session) {
			Ok(true) => {
				self.deleted_paths.insert(choice.session_path.clone());
				self.list.remove_session(&choice.session_path);
				self.folder_sessions
					.retain(|entry| entry.as_ref().path != choice.session_path);
				if let Some(global) = self.global_sessions.as_mut() {
					global.retain(|entry| entry.as_ref().path != choice.session_path);
				}
			}
			Ok(false) => {}
			Err(error) => self.show_error(&error),
		}
		self.request_render();
	}

	fn cancel_delete(&mut self) {
		self.delete_choice = None;
		self.request_render();
	}

	/// Open the rename editor over the selected session.
	fn show_rename_dialog(&mut self) {
		if self.on_rename.is_none() || self.delete_choice.is_some() || self.rename.is_some() || self.toggling {
			return;
		}
		let Some(session) = self.list.selected_session().cloned() else {
			return;
		};
		let mut input = Input::new();
		if let Some(title) = &session.as_ref().title {
			input.set_value(title.clone());
		}
		input.set_placeholder("Rename session");
		self.rename = Some(RenameState {
			session_path: session.as_ref().path.clone(),
			input,
		});
		self.request_render();
	}

	fn close_rename_dialog(&mut self) {
		if self.rename.is_some() {
			self.rename = None;
			self.request_render();
		}
	}

	/// Submit the rename editor's value.
	fn submit_rename(&mut self) {
		let Some(state) = self.rename.as_ref() else {
			return;
		};
		let title = state.input.value().trim().to_string();
		let path = state.session_path.clone();
		if title.is_empty() {
			self.close_rename_dialog();
			return;
		}
		self.clear_error();
		let Some(on_rename) = self.on_rename.as_mut() else {
			self.close_rename_dialog();
			return;
		};
		// Re-read the session from the folder listing, falling back to the
		// visible list, so the callback sees the current entry.
		let session = self
			.list
			.composed()
			.iter()
			.find(|entry| entry.as_ref().path == path)
			.cloned();
		let Some(session) = session else {
			self.close_rename_dialog();
			return;
		};

		match on_rename(&session, &title) {
			Ok(Some(stored)) => {
				self.list.rename_session_title(&path, &stored);
				self.renamed_titles.insert(path.clone(), stored.clone());
				for cache in [Some(&mut self.folder_sessions), self.global_sessions.as_mut()] {
					if let Some(cache) = cache {
						if let Some(entry) = cache
							.iter_mut()
							.find(|entry| entry.as_ref().path == path)
						{
							entry.as_mut().title = Some(stored.clone());
						}
					}
				}
			}
			Ok(None) => self.show_error("Failed to rename session."),
			Err(error) => self.show_error(&error),
		}
		self.close_rename_dialog();
	}

	/// Persist a pin change, then reorder the list.
	fn set_pinned(&mut self, session: &T, pinned: bool) {
		if self.on_set_pinned.is_none() {
			return;
		}
		self.clear_error();
		let result = match self.on_set_pinned.as_mut() {
			Some(callback) => callback(session, pinned),
			None => return,
		};
		match result {
			Ok(()) => self.list.set_pinned(&session.as_ref().id, pinned),
			Err(error) => self.show_error(&error),
		}
	}

	/// The pin action that applies to the selected session.
	#[must_use]
	pub fn pin_action_label(&self) -> Option<&'static str> {
		self.on_set_pinned.as_ref()?;
		Some(if self.list.selected_pinned() {
			"unpin"
		} else {
			"pin"
		})
	}

	/// The picker sheet for the current state.
	fn describe_picker(&self, now: i64) -> NativeNode {
		let view = self.list.picker_view(now);
		let preview_session = self.list.selected_session();
		let preview = preview_session
			.map(|session| {
				let forked = session
					.as_ref()
					.parent_session_path
					.as_deref()
					.map(|parent| self.list.parent_label(parent));
				session_preview(session.as_ref(), forked.as_deref())
			})
			.unwrap_or_default();
		let loading = self
			.message
			.as_ref()
			.is_some_and(|message| message.kind == SessionPickerMessageKind::Loading);
		let error_open = self
			.message
			.as_ref()
			.is_some_and(|message| message.kind == SessionPickerMessageKind::Error);
		let toggle = self.can_toggle_scope();
		let mut actions = vec![picker_action(
			"resume",
			&self.action_label,
			Some(crate::native::picker::PickerKeys::Id("enter")),
			PickerActionOptions {
				primary: true,
				..PickerActionOptions::default()
			},
		)];
		if self.on_delete.is_some() {
			actions.push(picker_action("delete", "Delete", None, PickerActionOptions::default()));
		}
		if self.on_rename.is_some() {
			actions.push(picker_action("rename", "Rename", None, PickerActionOptions::default()));
		}
		if let Some(label) = self.pin_action_label() {
			actions.push(picker_action(
				"pin",
				if label == "pin" { "Pin" } else { "Unpin" },
				None,
				PickerActionOptions::default(),
			));
		}
		if toggle {
			actions.push(picker_action(
				"scope",
				if self.scope == SessionSelectorScope::All {
					"This folder"
				} else {
					"All projects"
				},
				Some(crate::native::picker::PickerKeys::Id("tab")),
				PickerActionOptions::default(),
			));
		}
		actions.push(picker_action(
			"close",
			"Close",
			Some(crate::native::picker::PickerKeys::Id("escape")),
			PickerActionOptions {
				end: true,
				..PickerActionOptions::default()
			},
		));
		let rows: Vec<PickerItem> = view
			.items
			.iter()
			.map(|row| {
				let mut item = PickerItem::new(row.id.clone(), row.label.clone());
				if let Some(detail) = &row.detail {
					item.rest.set_str("detail", detail.as_str());
				}
				item.rest.insert(
					"facts",
					serde_json::json!({
						"when": row.facts[0].1,
						"size": row.facts[1].1,
					}),
				);
				if let Some(dot) = &row.dot {
					item.rest.set_str("dot", dot.as_str());
				}
				if row.pinned {
					item.rest.set_str("icon", "pin");
				}
				if !row.badges.is_empty() {
					let badges: Vec<serde_json::Value> = row
						.badges
						.iter()
						.map(|(text, tone)| {
							let mut badge = serde_json::Map::new();
							badge.insert("text".to_string(), serde_json::Value::String(text.clone()));
							if let Some(tone) = tone {
								badge.insert("tone".to_string(), serde_json::Value::String(tone.clone()));
							}
							serde_json::Value::Object(badge)
						})
						.collect();
					item.rest.insert("badges", serde_json::Value::Array(badges));
				}
				item
			})
			.collect();
		let confirm = self.delete_choice.as_ref().and_then(|choice| {
			let session = self
				.list
				.composed()
				.iter()
				.find(|session| session.as_ref().path == choice.session_path)?;
			Some(PickerConfirm {
				text: format!(
					"Delete “{}”? This removes the session file.",
					session_label(session.as_ref())
				),
				act: "delete-confirm".to_string(),
				label: "Delete".to_string(),
			})
		});
		let placeholder = if loading || self.scope == SessionSelectorScope::All {
			"Search all sessions…"
		} else {
			"Search sessions…"
		};
		let props = PickerProps {
			title: String::new(),
			subtitle: None,
			icon: Some("history".to_string()),
			noun: "sessions".to_string(),
			size: if self.standalone { "screen" } else { "lg" }.to_string(),
			layout: "cards".to_string(),
			preview: "side".to_string(),
			query: Some(view.query.clone()),
			cursor: Some(view.cursor),
			items: rows,
			selected: view.selected.clone(),
			current: Some(view.current.clone()),
			columns: Some(vec![
			PickerColumn {
				id: "when".to_string(),
				head: None,
				format: Some("time".to_string()),
				align: None,
				grow: None,
				priority: None,
			},
			PickerColumn {
				id: "size".to_string(),
				head: None,
				format: Some("dim".to_string()),
				align: None,
				grow: None,
				priority: None,
			},
			]),
			actions,
			confirm,
		};
		let mut node_props = props.to_props();
		node_props.set_str("placeholder", placeholder);
		node_props.set_i64("total", view.total as i64);
		node_props.set_str(
			"state",
			if loading {
				"loading"
			} else if error_open {
				"error"
			} else {
				"ready"
			},
		);
		if let Some(message) = self.message.as_ref().filter(|_| loading || error_open) {
			node_props.set_str("message", message.text.as_str());
		}
		node_props.set_str(
			"empty",
			if self.scope == SessionSelectorScope::All || !toggle {
				"No sessions yet"
			} else {
				"No sessions in this folder yet"
			},
		);
		let order: Vec<serde_json::Value> = view
			.order
			.iter()
			.map(|entry| match entry {
				SessionOrderEntry::Group { id, label, count } => serde_json::json!({
					"group": id,
					"label": label,
					"count": count,
				}),
				SessionOrderEntry::Row(path) => serde_json::Value::String(path.clone()),
			})
			.collect();
		node_props.insert("order", serde_json::Value::Array(order));
		if !view.hits.is_empty() {
			let hits: serde_json::Map<String, serde_json::Value> = view
				.hits
				.iter()
				.map(|(key, ranges)| {
					(
						key.clone(),
						serde_json::Value::Array(
							ranges
								.iter()
								.map(|(start, end)| serde_json::json!([start, end]))
								.collect(),
						),
					)
				})
				.collect();
			node_props.insert("hits", serde_json::Value::Object(hits));
		}
		node_props.insert(
			"preview",
			serde_json::Value::Array(preview.iter().map(NativeNode::to_prop_value).collect()),
		);
		let mut picker = node(NodeKind::Picker, node_props, Vec::new(), None);
		// The preview travels as child nodes so hosts can render it natively.
		picker.children = preview;
		picker
	}

	/// The card path: scope tabs, message, search input and list, key hints.
	fn describe_card(&self, now: i64) -> NativeNode {
		let mut children = Vec::new();
		let tabs = self.scope_label.is_none() && self.can_toggle_scope();
		if tabs {
			let mut props = Props::new();
			props.insert(
				"items",
				serde_json::json!(
					SCOPE_TABS
						.iter()
						.map(|(id, label)| serde_json::json!({ "id": id, "label": label }))
						.collect::<Vec<_>>()
				),
			);
			props.set_str("active", self.scope.as_str());
			children.push(node(NodeKind::Tabs, props, Vec::new(), Some("scope")));
		}
		if let Some(message) = &self.message {
			let mut props = Props::new();
			match message.kind {
				SessionPickerMessageKind::Loading => {
					props.insert(
						"label",
						serde_json::Value::Array(vec![span(message.text.clone(), Some("muted")).to_value()]),
					);
					children.push(node(NodeKind::Spinner, props, Vec::new(), Some("message")));
				}
				SessionPickerMessageKind::Error => {
					children.push(text(
						NodeText::Spans(vec![span(message.text.clone(), Some("error"))]),
						Props::new(),
					));
				}
			}
		}
		if self.delete_choice.is_some() {
			let mut props = Props::new();
			props.set_str("text", "Press enter to delete, escape to cancel");
			children.push(node(NodeKind::Card, props, Vec::new(), None));
		} else if self.rename.is_some() {
			let mut props = Props::new();
			props.set_str("label", "Rename session");
			children.push(node(NodeKind::Editor, props, Vec::new(), Some("rename")));
		} else {
			children.push(self.list.describe_list(&self.theme, now));
		}
		let mut hints: Vec<Option<crate::native::overlay::NativeHint>> = Vec::new();
		if self.on_delete.is_some() {
			hints.push(action_hint(
				&crate::keybindings::KeybindingsManager::default(),
				crate::native::overlay::Actions::from("app.session.delete"),
				"delete",
			));
		}
		children.push(hints_row(&hints, "hints"));
		overlay_card(
			self.native_role(),
			Some(NodeText::plain(self.header_label())),
			children,
			None,
		)
	}
}

impl<T: Clone + AsRef<SessionSelectorEntry> + AsMut<SessionSelectorEntry>> NativeDescribe for SessionSelectorComponent<T> {
	fn describe_native(&self, cx: &DescribeContext) -> NativeNode {
		let now = current_time_ms();
		if cx.feature("picker") && self.rename.is_none() {
			return self.describe_picker(now);
		}
		self.describe_card(now)
	}
}

impl<T: Clone + AsRef<SessionSelectorEntry> + AsMut<SessionSelectorEntry>> Component for SessionSelectorComponent<T> {
	fn render(&mut self, width: u16) -> Vec<String> {
		let inner = (width as usize).saturating_sub(4).max(1);
		let theme = OverlayTheme::default();
		let mut rows = vec![top_border(width, &self.header_label(), &theme), row("", width, &theme)];
		if let Some(message) = &self.message {
			rows.push(row(
				&truncate_to_width(&message.text, inner, Ellipsis::Unicode),
				width,
				&theme,
			));
		}
		if self.delete_choice.is_some() {
			rows.push(row(
				"Delete this session? enter delete · esc cancel",
				width,
				&theme,
			));
		} else if let Some(rename) = self.rename.as_mut() {
			for line in rename.input.render(inner as u16) {
				rows.push(row(&line, width, &theme));
			}
		} else {
			for line in self.list.render(inner as u16) {
				rows.push(row(&line, width, &theme));
			}
		}
		let scope_hint = if self.scope == SessionSelectorScope::All {
			"current folder"
		} else {
			"all projects"
		};
		let hint = format!("enter select · tab {scope_hint} · esc cancel");
		rows.push(row("", width, &theme));
		rows.push(row(&hint, width, &theme));
		rows.push(row("", width, &theme));
		rows.push(bottom_border(width, &theme));
		rows
	}

	fn handle_input(&mut self, key: &Key) {
		if self.input_locked {
			return;
		}
		if self.delete_choice.is_some() {
			match key {
				Key::Enter | Key::Char('y') | Key::Char('Y') => self.confirm_delete(),
				Key::Esc | Key::Char('n') | Key::Char('N') => self.cancel_delete(),
				_ => {}
			}
			return;
		}
		if let Some(rename) = self.rename.as_mut() {
			match key {
				Key::Enter => self.submit_rename(),
				Key::Esc => self.close_rename_dialog(),
				_ => rename.input.handle_input(key),
			}
			return;
		}
		match key {
			Key::Delete => {
				if let Some(session) = self.list.selected_session().cloned() {
					self.show_delete_confirmation(&session);
				}
				return;
			}
			Key::F(2) => {
				self.show_rename_dialog();
				return;
			}
			Key::Tab => {
				if self.can_toggle_scope() {
					let _ = self.toggle_scope();
				}
				return;
			}
			Key::Enter => {
				if let Some(session) = self.list.selected_session().cloned() {
					self.closed = true;
					(self.on_select)(&session);
				}
				return;
			}
			Key::Esc => {
				self.closed = true;
				(self.on_cancel)();
				return;
			}
			Key::Ctrl('c') => {
				self.closed = true;
				(self.on_exit)();
				return;
			}
			_ => {}
		}
		self.list.handle_input(key);
	}

	fn handle_mouse(&mut self, event: &crate::mouse::MouseEvent) {
		if self.delete_choice.is_some() || self.rename.is_some() {
			return;
		}
		self.list.handle_mouse(event);
	}
}

impl<T: Clone + AsRef<SessionSelectorEntry> + AsMut<SessionSelectorEntry>> SessionSelectorComponent<T> {
	/// Picker pointer events run the keys' paths.
	pub fn handle_native_event(&mut self, event: &NativeUiEvent) {
		if self.input_locked {
			return;
		}
		match event {
			NativeUiEvent::Action { act, .. } => match act.as_str() {
				"delete-confirm" => self.confirm_delete(),
				"cancel" => {
					if self.delete_choice.is_some() {
						self.cancel_delete();
					} else {
						self.close_rename_dialog();
					}
				}
				"close" => {
					if self.delete_choice.is_some() {
						self.cancel_delete();
					} else if self.rename.is_some() {
						self.close_rename_dialog();
					} else {
						self.closed = true;
						(self.on_cancel)();
					}
				}
				_ if self.delete_choice.is_some() || self.rename.is_some() => {}
				"resume" => {
					if let Some(session) = self.list.selected_session().cloned() {
						self.closed = true;
						(self.on_select)(&session);
					}
				}
				"delete" => {
					if let Some(session) = self.list.selected_session().cloned() {
						self.show_delete_confirmation(&session);
					}
				}
				"rename" => self.show_rename_dialog(),
				"pin" => {
					if let Some(session) = self.list.selected_session().cloned() {
						let pinned = self.list.selected_pinned();
						self.set_pinned(&session, !pinned);
					}
				}
				"scope" => {
					let _ = self.toggle_scope();
				}
				"clear" => self.list.clear_search(),
				_ => {}
			},
			NativeUiEvent::Activate { key, item } if key.is_empty() || key == &vec!["list".to_string()] => {
				if self.delete_choice.is_none() && self.rename.is_none() {
					self.list.confirm_session(item);
				}
			}
			NativeUiEvent::Select { key, item }
				if key == &vec!["scope".to_string()] && item != self.scope.as_str() =>
			{
				let _ = self.toggle_scope();
			}
			NativeUiEvent::Select { key, item }
				if key.is_empty() || key == &vec!["list".to_string()] =>
			{
				if self.delete_choice.is_none() && self.rename.is_none() {
					self.list.select_session(item);
				}
			}
			_ => {}
		}
	}
}

/// Keep the ANSI footer from drifting when a caller wants a full-height list.
#[must_use]
pub fn fill_height_rows(width: u16, rows: &[String], target: usize, theme: &OverlayTheme) -> Vec<String> {
	let mut out = rows.to_vec();
	if out.len() >= target {
		out.truncate(target);
		return out;
	}
	while out.len() < target {
		out.push(row("", width, theme));
	}
	out
}

#[cfg(test)]
mod tests {
	use super::*;

	fn entry(id: &str, title: Option<&str>, modified: i64, first: &str) -> SessionSelectorEntry {
		SessionSelectorEntry {
			path: format!("/tmp/{id}.jsonl"),
			id: id.to_string(),
			cwd: "/tmp".to_string(),
			title: title.map(str::to_string),
			modified,
			size: 2048,
			first_message: first.to_string(),
			all_messages_text: first.to_string(),
			..SessionSelectorEntry::default()
		}
	}

	#[test]
	fn titles_lead_then_recency_then_fuzzy() {
		let sessions = vec![
			entry("old", None, 1000, "needle in the haystack"),
			entry("new", None, 3000, "another needle mention"),
			entry("title", Some("exact needle"), 500, "unrelated"),
		];
		let ranked = rank_session_search_matches(&sessions, "needle");
		assert_eq!(ranked[0].id, "title");
		assert_eq!(ranked[1].id, "new");
		assert_eq!(ranked[2].id, "old");
	}

	#[test]
	fn history_matches_lead_and_duplicates_are_dropped() {
		let sessions = vec![
			entry("a", None, 1000, "alpha"),
			entry("b", None, 2000, "beta"),
		];
		let merged = merge_session_ranking(&sessions, sessions.clone(), &["b".to_string(), "b".to_string()]);
		assert_eq!(
			merged.iter().map(|entry| entry.id.as_str()).collect::<Vec<_>>(),
			vec!["b", "a"]
		);
	}

	#[test]
	fn session_labels_prefer_title_then_message_then_id() {
		assert_eq!(session_label(&entry("x", Some(" Title "), 0, "first")), "Title");
		assert_eq!(session_label(&entry("x", None, 0, " first\nmessage ")), "first message");
		let mut bare = entry("x", None, 0, "");
		bare.id = "fallback".to_string();
		assert_eq!(session_label(&bare), "fallback");
	}

	#[test]
	fn sessions_group_by_day() {
		let today = 20 * DAY_MS;
		assert_eq!(session_day_group(today + 5, today).id, "today");
		assert_eq!(session_day_group(today - 1, today).id, "yesterday");
		assert_eq!(session_day_group(today - 3 * DAY_MS, today).id, "week");
		assert_eq!(session_day_group(today - 30 * DAY_MS, today).id, "earlier");
	}

	#[test]
	fn ages_read_as_relative_times_then_dates() {
		let now = 1_700_000_000_000;
		assert_eq!(format_session_date(now - 30_000, now), "just now");
		assert_eq!(format_session_date(now - 120_000, now), "2 minutes ago");
		assert_eq!(format_session_date(now - 3_600_000, now), "1 hour ago");
		assert_eq!(format_session_date(now - 2 * DAY_MS, now), "2 days ago");
		assert_eq!(format_session_date(now - 30 * DAY_MS, now), "2023-09-24");
	}

	#[test]
	fn locales_fall_back_for_c_and_posix() {
		let mut env = HashMap::new();
		env.insert("LANG".to_string(), "C".to_string());
		assert_eq!(system_date_locale(&env), None);
		env.insert("LANG".to_string(), "en_GB.UTF-8".to_string());
		assert_eq!(system_date_locale(&env).as_deref(), Some("en-GB"));
	}

	fn list() -> SessionList<SessionSelectorEntry> {
		SessionList::new(
			vec![
				entry("a", None, 1000, "alpha session"),
				entry("b", None, 3000, "beta session"),
			],
			false,
			None,
			24,
			HashSet::new(),
			None,
		)
	}

	#[test]
	fn filtering_narrows_the_visible_list_and_resets_the_selection() {
		let mut list = list();
		list.filter_sessions("beta");
		assert_eq!(list.composed().len(), 1);
		assert_eq!(list.selected_index(), 0);
		assert_eq!(list.selected_session().unwrap().id, "b");
		list.clear_search();
		assert_eq!(list.composed().len(), 2);
	}

	#[test]
	fn pinned_sessions_lead_with_their_own_group() {
		let mut list = list();
		list.set_pinned("b", true);
		let composed = list.composed();
		assert_eq!(composed[0].id, "b");
		let view = list.picker_view(10_000);
		assert_eq!(
			view.order[0],
			SessionOrderEntry::Group {
				id: "pinned-0".to_string(),
				label: "Pinned".to_string(),
				count: 1
			}
		);
	}

	#[test]
	fn query_order_is_flat_and_hits_mark_the_label() {
		let mut list = list();
		list.filter_sessions("beta");
		let view = list.picker_view(10_000);
		assert_eq!(view.order.len(), 1);
		assert!(matches!(&view.order[0], SessionOrderEntry::Row(path) if path.ends_with("b.jsonl")));
		assert!(!view.hits.is_empty());
	}

	#[test]
	fn removing_and_renaming_sessions_updates_the_listing() {
		let mut list = list();
		list.remove_session("/tmp/a.jsonl");
		assert_eq!(list.sessions().len(), 1);
		list.rename_session_title("/tmp/b.jsonl", "Renamed");
		assert_eq!(
			list.sessions()[0].title.as_deref(),
			Some("Renamed")
		);
	}

	#[test]
	fn the_delete_flow_confirms_before_removing() {
		let theme = crate::theme::default_theme();
		let deleted = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
		let sink = std::rc::Rc::clone(&deleted);
		let mut options = SessionSelectorOptions {
			on_delete: Some(Box::new(move |session: &SessionSelectorEntry| {
				sink.borrow_mut().push(session.id.clone());
				Ok(true)
			})),
			..SessionSelectorOptions::default()
		};
		options.pinned_ids.clear();
		let mut selector = SessionSelectorComponent::new(
			vec![entry("a", None, 1000, "alpha")],
			|_| {},
			|| {},
			|| {},
			options,
			theme,
		);
		selector.handle_input(&Key::Delete);
		assert!(selector.delete_choice.is_some());
		selector.handle_input(&Key::Enter);
		assert!(selector.delete_choice.is_none());
		assert_eq!(&*deleted.borrow(), &["a".to_string()]);
		assert_eq!(selector.session_list().sessions().len(), 0);
	}

	#[test]
	fn cancelling_the_delete_keeps_the_session() {
		let theme = crate::theme::default_theme();
		let mut selector = SessionSelectorComponent::new(
			vec![entry("a", None, 1000, "alpha")],
			|_| {},
			|| {},
			|| {},
			SessionSelectorOptions {
				on_delete: Some(Box::new(|_session: &SessionSelectorEntry| Ok(true))),
				..SessionSelectorOptions::default()
			},
			theme,
		);
		selector.handle_input(&Key::Delete);
		selector.handle_input(&Key::Esc);
		assert!(selector.delete_choice.is_none());
		assert_eq!(selector.session_list().sessions().len(), 1);
	}

	#[test]
	fn renaming_updates_the_row_and_the_scope_cache() {
		let theme = crate::theme::default_theme();
		let mut selector = SessionSelectorComponent::new(
			vec![entry("a", None, 1000, "alpha")],
			|_| {},
			|| {},
			|| {},
			SessionSelectorOptions {
				on_rename: Some(Box::new(|_session: &SessionSelectorEntry, title: &str| {
					Ok(Some(title.to_string()))
				})),
				..SessionSelectorOptions::default()
			},
			theme,
		);
		selector.handle_input(&Key::F(2));
		assert!(selector.rename.is_some());
		selector.rename.as_mut().unwrap().input.set_value("New title");
		selector.handle_input(&Key::Enter);
		assert!(selector.rename.is_none());
		assert_eq!(
			selector.folder_sessions[0].title.as_deref(),
			Some("New title")
		);
	}

	#[test]
	fn scope_toggle_loads_all_projects_once() {
		let theme = crate::theme::default_theme();
		let loads = std::rc::Rc::new(std::cell::RefCell::new(0));
		let sink = std::rc::Rc::clone(&loads);
		let mut selector = SessionSelectorComponent::new(
			vec![entry("a", None, 1000, "alpha")],
			|_| {},
			|| {},
			|| {},
			SessionSelectorOptions {
				load_all_sessions: Some(Box::new(move || {
					*sink.borrow_mut() += 1;
					Ok(vec![entry("g", None, 2000, "global")])
				})),
				..SessionSelectorOptions::default()
			},
			theme,
		);
		selector.handle_input(&Key::Tab);
		assert_eq!(selector.scope(), SessionSelectorScope::All);
		assert_eq!(selector.session_list().sessions()[0].id, "g");
		selector.handle_input(&Key::Tab);
		assert_eq!(selector.scope(), SessionSelectorScope::Folder);
		assert_eq!(*loads.borrow(), 1);
	}

	#[test]
	fn the_picker_carries_the_confirm_strip_and_actions() {
		let theme = crate::theme::default_theme();
		let mut selector = SessionSelectorComponent::new(
			vec![entry("a", Some("Alpha"), 1000, "alpha")],
			|_| {},
			|| {},
			|| {},
			SessionSelectorOptions {
				on_delete: Some(Box::new(|_session: &SessionSelectorEntry| Ok(true))),
				..SessionSelectorOptions::default()
			},
			theme,
		);
		selector.handle_input(&Key::Delete);
		let mut cx = DescribeContext::new(120);
		cx.enable_feature("picker");
		let described = selector.describe_native(&cx);
		assert_eq!(described.kind, NodeKind::Picker);
		let confirm = described.props.get("confirm").expect("confirm strip");
		assert_eq!(confirm.get("act").and_then(|act| act.as_str()), Some("delete-confirm"));
		let actions = described
			.props
			.get("actions")
			.and_then(|actions| actions.as_array())
			.expect("actions");
		assert!(actions.iter().any(|action| {
			action.get("id").and_then(|id| id.as_str()) == Some("delete")
		}));
	}
}
