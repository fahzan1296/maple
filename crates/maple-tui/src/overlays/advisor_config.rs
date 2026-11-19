//! Advisor configuration overlay data, callbacks, and pure helpers.
//!
//! Reference: `l/src/overlays/advisor-config.ts`.
//!
//! Deviations:
//! - The fullscreen TUI component (`AdvisorConfigOverlayComponent`), screen
//!   navigation/rendering, key/mouse handling, `ModelSelectorComponent`,
//!   `HookEditorComponent`, `EditorSheet`, `SelectList`/`Input` integration and
//!   theme/chrome drawing are not ported (no `TUI`/component runtime in crate).
//!   This module ports the exported data types, constants, screen model, and
//!   pure/helper functions and defines host callback/dep traits for runtime
//!   integration.
//! - `format_compact_quota` takes precomputed `UsageLimitView` data and a
//!   `format_duration_ms` formatter injected by host because `@oh-my-pi/pi-ai`
//!   `resolveUsedFraction`/report types and `formatDuration` utilities are not
//!   present. Quota math/selection logic is preserved.
//! - `ThinkingLevel`/`Model`/keybinding hint types are represented as strings
//!   or opaque values where concrete crate types are unavailable.

use std::collections::BTreeSet;

/// Review modes.
pub const ADVISOR_REVIEW_MODES: &[&str] = &["turn", "agent-end"];
/// Sync backlog modes.
pub const ADVISOR_SYNC_BACKLOG_MODES: &[&str] = &["off", "1", "3", "5", "strict"];
/// Session scopes.
pub const ADVISOR_SESSION_SCOPES: &[&str] = &["main", "subagent", "all"];

pub type AdvisorReviewMode = String;
pub type AdvisorSyncBacklog = String;
pub type AdvisorSessionScope = String;

/// Choice value for "no per-advisor override"; matches no sync backlog mode entry.
pub const SYNC_BACKLOG_INHERIT: &str = "inherit";
/// The tools list's terminal-only "Done" row.
pub const TOOLS_DONE: &str = "__done";
/// Preview width for instruction first-line previews.
pub const PREVIEW_WIDTH: usize = 60;

/// One advisor declared in `WATCHDOG.yml`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AdvisorConfig {
	pub name: String,
	/// Model selector with optional `:level` thinking suffix.
	pub model: Option<String>,
	/// Built-in tool names; omitted uses defaults, empty grants none.
	pub tools: Option<Vec<String>>,
	pub instructions: Option<String>,
	/// Defaults to true at interpretation time.
	pub enabled: Option<bool>,
	/// Max non-blocker notes per update; blockers exempt.
	pub max_notes_per_update: Option<u32>,
	pub review_mode: Option<AdvisorReviewMode>,
	pub review_interval: Option<u32>,
	pub sync_backlog: Option<AdvisorSyncBacklog>,
	/// `main` | `subagent` | `all` (defaults to all).
	pub scope: Option<AdvisorSessionScope>,
	/// Whether primary assistant thinking blocks are included (defaults true).
	pub include_reasoning: Option<bool>,
}

/// Which level a `WATCHDOG.yml` lives at.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AdvisorConfigScope {
	Project,
	User,
}

/// Editable raw contents of one `WATCHDOG.yml`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WatchdogConfigDoc {
	pub instructions: Option<String>,
	pub max_notes_per_update: Option<u32>,
	pub advisors: Vec<AdvisorConfig>,
	/// Per-entry problems found while loading (dropped entries).
	pub warnings: Option<Vec<String>>,
}

/// Live advisor usage stat shown in preview.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AdvisorConfigStat {
	pub name: String,
	pub session_id: Option<String>,
	pub status: String,
	pub model_provider: Option<String>,
	pub tokens_input: u64,
	pub tokens_output: u64,
	pub tokens_cache_read: u64,
	pub cost: f64,
	pub context_window: u64,
	pub context_tokens: u64,
}

/// Host callbacks for advisor config persistence/runtime effects.
pub trait AdvisorConfigCallbacks {
	fn load_doc(&self, scope: AdvisorConfigScope) -> Result<WatchdogConfigDoc, String>;
	fn save(&self, scope: AdvisorConfigScope, doc: &WatchdogConfigDoc) -> Result<(), String>;
	fn close(&self);
	fn request_render(&self);
	fn notify(&self, message: &str);
	fn warn(&self, message: &str) {
		self.notify(message);
	}
	fn get_advisor_stats(&self) -> Vec<AdvisorConfigStat> {
		Vec::new()
	}
}

/// Minimal usage window view for quota formatting.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UsageWindowView {
	pub id: Option<String>,
	pub label: Option<String>,
	pub resets_at_ms: Option<i64>,
	pub reset_label: Option<String>,
}

/// Minimal usage limit view for quota formatting.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct UsageLimitView {
	pub window: Option<UsageWindowView>,
	pub scope_window_id: Option<String>,
	pub label: String,
	/// Used fraction 0..1 (host-resolved, equivalent to TS `resolveUsedFraction`).
	pub used_fraction: Option<f64>,
}

/// Minimal usage report view for quota formatting.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct UsageReportView {
	pub provider: String,
	pub limits: Vec<UsageLimitView>,
}

/// Runtime dependencies for the overlay (UI/model/tool resolution supplied by host).
pub trait AdvisorConfigDeps {
	fn available_tool_names(&self) -> Vec<String>;
	fn default_tool_names(&self) -> BTreeSet<String>;
	fn sync_backlog(&self) -> Option<AdvisorSyncBacklog> {
		None
	}
	fn default_model_label(&self) -> Option<String> {
		None
	}
}

/// Overlay screens.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Screen {
	List,
	Detail,
	Name,
	Model,
	Tools,
	Thinking,
	ReviewMode,
	ReviewInterval,
	SyncBacklog,
	Instructions,
}

/// Footer hint (logical form).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeHint {
	pub keys: Vec<String>,
	pub label: String,
}

impl NativeHint {
	#[must_use]
	pub fn new(keys: Vec<String>, label: impl Into<String>) -> Self {
		Self { keys, label: label.into() }
	}
}

fn action_hint(action: &str, label: &str) -> NativeHint {
	NativeHint::new(vec![action.to_string()], label.to_string())
}

/// Footer key hints per screen.
#[must_use]
pub fn screen_hints(screen: Screen) -> Vec<NativeHint> {
	match screen {
		Screen::List => vec![
			action_hint("tui.select.up/down", "move"),
			action_hint("tui.select.confirm", "select"),
			action_hint("tui.select.cancel", "close"),
		],
		Screen::Detail => vec![
			action_hint("tui.select.confirm", "edit field"),
			action_hint("tui.select.cancel", "back"),
		],
		Screen::Name => vec![
			action_hint("tui.input.submit", "save"),
			action_hint("tui.select.cancel", "cancel"),
		],
		Screen::Model => vec![
			NativeHint::new(vec![], "type to search"),
			NativeHint::new(vec!["enter".into()], "pick"),
			action_hint("tui.select.cancel", "back"),
		],
		Screen::Thinking | Screen::ReviewMode | Screen::SyncBacklog => vec![
			action_hint("tui.select.confirm", "pick"),
			action_hint("tui.select.cancel", "back"),
		],
		Screen::ReviewInterval => vec![
			action_hint("tui.input.submit", "save"),
			action_hint("tui.select.cancel", "cancel"),
		],
		Screen::Tools => vec![
			action_hint("tui.select.confirm", "toggle"),
			NativeHint::new(vec![], "Done applies (empty = no tools; read/grep/glob = default)"),
			action_hint("tui.select.cancel", "apply"),
		],
		Screen::Instructions => vec![],
	}
}

/// One-line provider quota display.
#[must_use]
pub fn format_compact_quota<F>(
	provider: &str,
	reports: &[UsageReportView],
	now_ms: i64,
	include_limit: Option<&dyn Fn(&UsageReportView, &UsageLimitView) -> bool>,
	format_duration_ms: F,
) -> Option<String>
where
	F: Fn(i64) -> String,
{
	use std::collections::HashMap;
	let mut by_window: HashMap<String, (UsageLimitView, f64)> = HashMap::new();
	for report in reports {
		if report.provider != provider {
			continue;
		}
		for limit in &report.limits {
			if let Some(pred) = include_limit {
				if !pred(report, limit) {
					continue;
				}
			}
			let Some(fraction) = limit.used_fraction else {
				continue;
			};
			let key = limit
				.window
				.as_ref()
				.and_then(|w| w.id.clone())
				.or_else(|| limit.scope_window_id.clone())
				.unwrap_or_else(|| "—".to_string());
			match by_window.get(&key) {
				Some((_, existing)) if *existing >= fraction => {}
				_ => {
					by_window.insert(key, (limit.clone(), fraction));
				}
			}
		}
	}
	if by_window.is_empty() {
		return None;
	}
	let mut entries: Vec<_> = by_window.into_values().collect();
	entries.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
	let mut lines = Vec::new();
	for (limit, fraction) in entries {
		let pct = (fraction * 100.0).round() as i64;
		let window_label = limit
			.window
			.as_ref()
			.and_then(|w| w.label.clone())
			.or_else(|| limit.scope_window_id.clone())
			.unwrap_or_else(|| "—".to_string());
		let identity = limit.label.trim();
		let header = if !identity.is_empty() && identity != window_label {
			format!("{window_label} ({identity})")
		} else {
			window_label
		};
		let mut parts = vec![format!("{header}: {pct}% used")];
		if let Some(window) = &limit.window {
			if let Some(resets_at) = window.resets_at_ms {
				if resets_at > now_ms {
					let reset_label = window.reset_label.as_deref().unwrap_or("resets");
					parts.push(format!(
						"{reset_label} in {}",
						format_duration_ms(resets_at - now_ms)
					));
				}
			}
		}
		lines.push(parts.join(" · "));
	}
	Some(format!("Quota: {}", lines.join(" │ ")))
}

fn is_finite_i64(v: i64) -> bool {
	// i64 always finite; keep parity with TS Number.isFinite guard intent.
	let _ = v;
	true
}

/// First-line preview or `(none)`.
#[must_use]
pub fn preview_line_or_none(text: Option<&str>) -> String {
	let Some(text) = text else {
		return "(none)".to_string();
	};
	if text.trim().is_empty() {
		return "(none)".to_string();
	}
	let first = text.trim().split('\n').next().unwrap_or("");
	if first.chars().count() > PREVIEW_WIDTH {
		let take = PREVIEW_WIDTH.saturating_sub(1);
		let truncated: String = first.chars().take(take).collect();
		format!("{truncated}…")
	} else {
		first.to_string()
	}
}

/// Omitted means default read/grep/glob; explicit empty set means no tools.
#[must_use]
pub fn commit_tools(
	selected: &BTreeSet<String>,
	all: &[String],
	defaults: &BTreeSet<String>,
) -> Option<Vec<String>> {
	if selected.is_empty() {
		return Some(Vec::new());
	}
	if selected.len() == defaults.len() && defaults.iter().all(|n| selected.contains(n)) {
		return None;
	}
	Some(all.iter().filter(|n| selected.contains(*n)).cloned().collect())
}

/// Format advisor tools for display.
#[must_use]
pub fn format_advisor_tools(tools: Option<&[String]>, empty_label: &str) -> String {
	match tools {
		None => "read, grep, glob (default)".to_string(),
		Some(t) if t.is_empty() => empty_label.to_string(),
		Some(t) => t.join(", "),
	}
}

/// One-line cadence label: mode plus interval.
#[must_use]
pub fn format_review_cadence(advisor: &AdvisorConfig) -> String {
	let review_interval = advisor.review_interval.unwrap_or(1);
	let mode = advisor.review_mode.as_deref().unwrap_or("turn");
	let suffix = if review_interval == 1 { "" } else { "s" };
	format!("{mode} · every {review_interval} eligible update{suffix}")
}

/// Picker description for one explicit catch-up policy value.
#[must_use]
pub fn describe_sync_backlog_mode(mode: &str) -> String {
	match mode {
		"off" => "Never wait for this advisor's backlog.".to_string(),
		"strict" => "Wait for all of this advisor's scheduled reviews; no wall-clock cap.".to_string(),
		_ => format!("Wait until fewer than {mode} scheduled reviews remain (30s cap)."),
	}
}

/// Soft-wrap plain text to `width`, returning at least one (possibly empty) line.
#[must_use]
pub fn wrap_text(text: &str, width: usize) -> Vec<String> {
	if text.is_empty() {
		return vec![String::new()];
	}
	let width = width.max(1);
	let mut lines = Vec::new();
	for para in text.split('\n') {
		if para.is_empty() {
			lines.push(String::new());
			continue;
		}
		let chars: Vec<char> = para.chars().collect();
		let mut i = 0;
		while i < chars.len() {
			let end = (i + width).min(chars.len());
			lines.push(chars[i..end].iter().collect());
			i = end;
		}
	}
	if lines.is_empty() {
		vec![String::new()]
	} else {
		lines
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn preview_line_or_none_truncates_first_line() {
		assert_eq!(preview_line_or_none(None), "(none)");
		assert_eq!(preview_line_or_none(Some("  \n")), "(none)");
		assert_eq!(preview_line_or_none(Some("hello\nworld")), "hello");
		let long = "x".repeat(PREVIEW_WIDTH + 10);
		let p = preview_line_or_none(Some(&long));
		assert!(p.ends_with('…'));
		assert_eq!(p.chars().count(), PREVIEW_WIDTH);
	}

	#[test]
	fn commit_tools_semantics_match_reference() {
		let defaults: BTreeSet<String> = ["glob", "grep", "read"].into_iter().map(String::from).collect();
		let all = vec!["glob".into(), "grep".into(), "read".into(), "write".into()];
		assert_eq!(commit_tools(&BTreeSet::new(), &all, &defaults), Some(vec![]));
		let selected_all_defaults = defaults.clone();
		assert_eq!(commit_tools(&selected_all_defaults, &all, &defaults), None);
		let mut custom = BTreeSet::new();
		custom.insert("read".into());
		custom.insert("write".into());
		assert_eq!(commit_tools(&custom, &all, &defaults), Some(vec!["read".into(), "write".into()]));
	}

	#[test]
	fn format_advisor_tools_and_cadence() {
		assert_eq!(format_advisor_tools(None, "none"), "read, grep, glob (default)");
		assert_eq!(format_advisor_tools(Some(&[]), "none"), "none");
		assert_eq!(format_advisor_tools(Some(&["a".into(), "b".into()]), "none"), "a, b");
		let a = AdvisorConfig { review_mode: Some("agent-end".into()), review_interval: Some(2), ..Default::default() };
		assert_eq!(format_review_cadence(&a), "agent-end · every 2 eligible updates");
		let b = AdvisorConfig::default();
		assert_eq!(format_review_cadence(&b), "turn · every 1 eligible update");
	}

	#[test]
	fn sync_backlog_and_wrap_and_hints() {
		assert_eq!(describe_sync_backlog_mode("off"), "Never wait for this advisor's backlog.");
		assert!(describe_sync_backlog_mode("3").contains("fewer than 3"));
		assert_eq!(wrap_text("", 10), vec![String::new()]);
		assert_eq!(wrap_text("abcd", 2), vec!["ab".to_string(), "cd".to_string()]);
		assert!(!screen_hints(Screen::List).is_empty());
		assert!(screen_hints(Screen::Instructions).is_empty());
	}

	#[test]
	fn format_compact_quota_selects_max_fraction_and_formats_lines() {
		let reports = vec![UsageReportView {
			provider: "p".into(),
			limits: vec![
				UsageLimitView {
					window: Some(UsageWindowView { id: Some("w".into()), label: Some("Hour".into()), resets_at_ms: Some(5000), reset_label: Some("resets".into()) }),
					scope_window_id: None,
					label: "Hour".into(),
					used_fraction: Some(0.42),
				},
				UsageLimitView {
					window: Some(UsageWindowView { id: Some("w".into()), label: Some("Hour".into()), resets_at_ms: None, reset_label: None }),
					scope_window_id: None,
					label: "Hour".into(),
					used_fraction: Some(0.10),
				},
			],
		}];
		let out = format_compact_quota("p", &reports, 1000, None, |_| "4s".to_string()).expect("quota");
		assert!(out.starts_with("Quota: "));
		assert!(out.contains("42% used"));
		assert!(out.contains("resets in 4s"));
		assert!(format_compact_quota("other", &reports, 0, None, |_| "x".into()).is_none());
	}
}
// -----------------------------------------------------------------------------
// Component
// -----------------------------------------------------------------------------

use crate::chrome::overlay_box::{bottom_border, divider, row, top_border, OverlayTheme};
use crate::component::Component;
use crate::keybindings::KeybindingsManager;
use crate::keys::{self, Key};
use crate::mouse::MouseEvent;

/// Fullscreen advisor-configuration overlay component.
///
/// Reference: `AdvisorConfigOverlayComponent` in `l/src/overlays/advisor-config.ts`.
///
/// Deviations: host UI subsystems (TUI split pane, SelectList/ModelBrowser/
/// HookEditor/EditorSheet, native prefs describe, theme singleton, usage-report
/// prefetch timer) are injected or reduced to state-driven text rendering.
/// Navigation/state transitions (list/detail screens, add/remove/save, dirty
/// tracking, selection, close/back, hints) are implemented here.
pub struct AdvisorConfigOverlayComponent<C: AdvisorConfigCallbacks, D: AdvisorConfigDeps> {
	pub scope: AdvisorConfigScope,
	pub doc: WatchdogConfigDoc,
	pub dirty: bool,
	pub screen: Screen,
	pub selected: usize,
	pub detail_index: usize,
	pub preview_scroll: usize,
	pub status: Option<String>,
	callbacks: C,
	deps: D,
	bindings: KeybindingsManager,
	theme: OverlayTheme,
}

impl<C: AdvisorConfigCallbacks, D: AdvisorConfigDeps> std::fmt::Debug
	for AdvisorConfigOverlayComponent<C, D>
{
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("AdvisorConfigOverlayComponent")
			.field("scope", &self.scope)
			.field("screen", &self.screen)
			.field("dirty", &self.dirty)
			.finish_non_exhaustive()
	}
}

impl<C: AdvisorConfigCallbacks, D: AdvisorConfigDeps> AdvisorConfigOverlayComponent<C, D> {
	#[must_use]
	pub fn new(scope: AdvisorConfigScope, doc: WatchdogConfigDoc, callbacks: C, deps: D) -> Self {
		Self {
			scope,
			doc,
			dirty: false,
			screen: Screen::List,
			selected: 0,
			detail_index: 0,
			preview_scroll: 0,
			status: None,
			callbacks,
			deps,
			bindings: KeybindingsManager::new(),
			theme: OverlayTheme::default(),
		}
	}

	pub fn set_keybindings(&mut self, bindings: KeybindingsManager) {
		self.bindings = bindings;
	}

	pub fn set_theme(&mut self, theme: OverlayTheme) {
		self.theme = theme;
	}

	#[must_use]
	pub fn title(&self) -> String {
		format!(
			"Advisor configuration · {:?}{}",
			self.scope,
			if self.dirty { "  ● unsaved" } else { "" }
		)
	}

	#[must_use]
	pub fn advisors(&self) -> &[AdvisorConfig] {
		&self.doc.advisors
	}

	fn clamp_selection(&mut self) {
		let count = self.doc.advisors.len();
		if count == 0 {
			self.selected = 0;
			self.detail_index = 0;
			return;
		}
		if self.selected >= count {
			self.selected = count - 1;
		}
		if self.detail_index >= count {
			self.detail_index = count - 1;
		}
	}

	fn list_lines(&self, width: usize) -> Vec<String> {
		let mut lines = vec![format!("Scope: {:?}", self.scope), "Advisors:".to_string()];
		if self.doc.advisors.is_empty() {
			lines.push("  (none)  ·  press a to add".to_string());
		}
		for (i, a) in self.doc.advisors.iter().enumerate() {
			let cursor = if i == self.selected { "›" } else { " " };
			let name = if a.name.trim().is_empty() { "(unnamed)" } else { a.name.as_str() };
			let default_model = self.deps.default_model_label();
			let model = a
				.model
				.as_deref()
				.or(default_model.as_deref())
				.unwrap_or("advisor role default");
			lines.push(format!("{cursor} {name}  —  {model}"));
		}
		if let Some(shared) = &self.doc.instructions {
			lines.push(String::new());
			lines.push("Shared instructions:".to_string());
			for l in wrap_text(&preview_line_or_none(Some(shared)), width.max(20)) {
				lines.push(format!("  {l}"));
			}
		}
		if let Some(warnings) = &self.doc.warnings {
			for w in warnings {
				lines.push(format!("⚠ {w}"));
			}
		}
		lines
	}

	fn detail_lines(&self, width: usize) -> Vec<String> {
		let mut lines = Vec::new();
		let Some(a) = self.doc.advisors.get(self.detail_index) else {
			lines.push("No advisor selected".to_string());
			return lines;
		};
		lines.push(format!("Advisor: {}", if a.name.is_empty() { "(unnamed)" } else { &a.name }));
		lines.push(format!("Enabled: {}", a.enabled.unwrap_or(true)));
		let default_model = self.deps.default_model_label();
		lines.push(format!(
			"Model: {}",
			a.model
				.as_deref()
				.or(default_model.as_deref())
				.unwrap_or("advisor role default")
		));
		lines.push(format!("Tools: {}", format_advisor_tools(a.tools.as_deref(), "no tools")));
		lines.push(format!("Cadence: {}", format_review_cadence(a)));
		let default_backlog = self.deps.sync_backlog();
		let sync = a
			.sync_backlog
			.as_deref()
			.or(default_backlog.as_deref())
			.unwrap_or("inherit/default");
		lines.push(format!("Sync backlog: {sync}"));
		lines.push(format!("Scope: {}", a.scope.as_deref().unwrap_or("all")));
		lines.push(format!("Include reasoning: {}", a.include_reasoning.unwrap_or(true)));
		lines.push(String::new());
		lines.push("Instructions:".to_string());
		for l in wrap_text(&preview_line_or_none(a.instructions.as_deref()), width.max(20)) {
			lines.push(format!("  {l}"));
		}
		lines
	}

	fn screen_lines(&self, width: usize) -> Vec<String> {
		match self.screen {
			Screen::List => self.list_lines(width),
			Screen::Detail => self.detail_lines(width),
			Screen::Name => vec![
				"Edit name (host input)".into(),
				self.doc.advisors.get(self.detail_index).map(|a| a.name.clone()).unwrap_or_default(),
			],
			Screen::Model => vec![
				"Select model (host picker)".into(),
				self.doc.advisors.get(self.detail_index).and_then(|a| a.model.clone()).unwrap_or_default(),
			],
			Screen::Tools => vec![
				"Edit tools (host picker)".into(),
				format_advisor_tools(
					self.doc.advisors.get(self.detail_index).and_then(|a| a.tools.as_deref()),
					"no tools",
				),
			],
			Screen::Thinking => vec!["Select thinking level (host picker)".into()],
			Screen::ReviewMode => vec![
				"Select review mode (host picker)".into(),
				self.doc
					.advisors
					.get(self.detail_index)
					.and_then(|a| a.review_mode.clone())
					.unwrap_or_else(|| "turn".into()),
			],
			Screen::ReviewInterval => vec![
				"Edit review interval (host input)".into(),
				self.doc
					.advisors
					.get(self.detail_index)
					.and_then(|a| a.review_interval)
					.unwrap_or(1)
					.to_string(),
			],
			Screen::SyncBacklog => vec![
				"Select sync backlog (host picker)".into(),
				self.doc
					.advisors
					.get(self.detail_index)
					.and_then(|a| a.sync_backlog.clone())
					.unwrap_or_else(|| SYNC_BACKLOG_INHERIT.into()),
			],
			Screen::Instructions => vec![
				"Edit instructions (host editor)".into(),
				preview_line_or_none(self.doc.instructions.as_deref()),
			],
		}
	}

	fn footer_text(&self) -> String {
		let hints = screen_hints(self.screen);
		let mut parts: Vec<String> = hints
			.iter()
			.map(|h| {
				let keys = if h.keys.is_empty() {
					String::new()
				} else {
					format!("{} ", h.keys.join("/"))
				};
				format!("{keys}{}", h.label)
			})
			.collect();
		if let Some(s) = &self.status {
			parts.push(s.clone());
		}
		parts.join(" · ")
	}

	pub fn open_detail(&mut self) {
		self.detail_index = self.selected;
		self.screen = Screen::Detail;
	}

	pub fn back(&mut self) -> bool {
		match self.screen {
			Screen::List => false,
			Screen::Detail => {
				self.screen = Screen::List;
				true
			}
			_ => {
				self.screen = Screen::Detail;
				true
			}
		}
	}

	pub fn close(&mut self) {
		self.callbacks.close();
	}

	pub fn save(&mut self) {
		match self.callbacks.save(self.scope, &self.doc) {
			Ok(()) => {
				self.dirty = false;
				self.status = Some("Saved".to_string());
				self.callbacks.notify("Advisor config saved");
			}
			Err(e) => {
				self.status = Some(format!("Save failed: {e}"));
				self.callbacks.warn(&format!("Advisor config save failed: {e}"));
			}
		}
	}

	pub fn add_advisor(&mut self) {
		self.doc.advisors.push(AdvisorConfig {
			name: format!("advisor-{}", self.doc.advisors.len() + 1),
			..Default::default()
		});
		self.selected = self.doc.advisors.len().saturating_sub(1);
		self.detail_index = self.selected;
		self.dirty = true;
		self.screen = Screen::Detail;
	}

	pub fn remove_selected(&mut self) {
		if self.doc.advisors.is_empty() {
			return;
		}
		let idx = self.selected.min(self.doc.advisors.len() - 1);
		self.doc.advisors.remove(idx);
		self.clamp_selection();
		self.dirty = true;
		if self.doc.advisors.is_empty() {
			self.screen = Screen::List;
		}
	}

	fn move_selection(&mut self, delta: isize) {
		let count = self.doc.advisors.len();
		if count == 0 {
			self.selected = 0;
			return;
		}
		let cur = self.selected as isize;
		let next = (cur + delta).clamp(0, count.saturating_sub(1) as isize) as usize;
		self.selected = next;
	}

	fn render_frame(&self, width: u16) -> Vec<String> {
		let title = self.title();
		let inner_w = width.saturating_sub(4) as usize;
		let mut body = self.screen_lines(inner_w.max(10));
		if self.preview_scroll < body.len() {
			body = body.split_off(self.preview_scroll);
		} else if !body.is_empty() {
			body.clear();
		}
		let footer = self.footer_text();
		let mut out = Vec::new();
		out.push(top_border(width, &title, &self.theme));
		for line in body {
			out.push(row(&line, width, &self.theme));
		}
		out.push(divider(width, &self.theme));
		out.push(row(&footer, width, &self.theme));
		out.push(bottom_border(width, &self.theme));
		out
	}
}

impl<C: AdvisorConfigCallbacks, D: AdvisorConfigDeps> Component
	for AdvisorConfigOverlayComponent<C, D>
{
	fn render(&mut self, width: u16) -> Vec<String> {
		self.render_frame(width)
	}

	fn handle_input(&mut self, key: &Key) {
		if self.screen == Screen::List {
			if keys::is_arrow_up(key) || self.bindings.matches(key, "tui.select.up") {
				self.move_selection(-1);
				self.callbacks.request_render();
				return;
			}
			if keys::is_arrow_down(key) || self.bindings.matches(key, "tui.select.down") {
				self.move_selection(1);
				self.callbacks.request_render();
				return;
			}
			if keys::is_enter(key) || self.bindings.matches(key, "tui.select.confirm") {
				self.open_detail();
				self.callbacks.request_render();
				return;
			}
			if keys::is_escape(key) || self.bindings.matches(key, "tui.select.cancel") {
				self.close();
				return;
			}
			if matches!(key, Key::Char('a')) {
				self.add_advisor();
				self.callbacks.request_render();
				return;
			}
			if matches!(key, Key::Char('s')) {
				self.save();
				self.callbacks.request_render();
				return;
			}
			if keys::is_delete(key) || keys::is_backspace(key) {
				self.remove_selected();
				self.callbacks.request_render();
				return;
			}
		} else if keys::is_escape(key) || self.bindings.matches(key, "tui.select.cancel") {
			if !self.back() {
				self.close();
			}
			self.callbacks.request_render();
		} else if keys::is_enter(key) || self.bindings.matches(key, "tui.select.confirm") {
			self.screen = Screen::Detail;
			self.callbacks.request_render();
		}
	}

	fn handle_mouse(&mut self, event: &MouseEvent) {
		if event.is_scroll_up() {
			self.preview_scroll = self.preview_scroll.saturating_sub(1);
			self.callbacks.request_render();
		} else if event.is_scroll_down() {
			self.preview_scroll = self.preview_scroll.saturating_add(1);
			self.callbacks.request_render();
		}
	}
}

#[cfg(test)]
mod component_tests {
	use super::*;
	use std::cell::RefCell;

	#[derive(Default)]
	struct Cb {
		closed: RefCell<bool>,
		saved: RefCell<bool>,
		rendered: RefCell<usize>,
	}
	impl AdvisorConfigCallbacks for Cb {
		fn load_doc(&self, _scope: AdvisorConfigScope) -> Result<WatchdogConfigDoc, String> {
			Ok(WatchdogConfigDoc::default())
		}
		fn save(&self, _scope: AdvisorConfigScope, _doc: &WatchdogConfigDoc) -> Result<(), String> {
			*self.saved.borrow_mut() = true;
			Ok(())
		}
		fn close(&self) {
			*self.closed.borrow_mut() = true;
		}
		fn request_render(&self) {
			*self.rendered.borrow_mut() += 1;
		}
		fn notify(&self, _message: &str) {}
	}
	#[derive(Default)]
	struct Deps;
	impl AdvisorConfigDeps for Deps {
		fn available_tool_names(&self) -> Vec<String> {
			vec!["read".into(), "grep".into(), "glob".into()]
		}
		fn default_tool_names(&self) -> BTreeSet<String> {
			["read".into(), "grep".into(), "glob".into()].into_iter().collect()
		}
	}

	#[test]
	fn advisor_config_component_renders_and_navigates() {
		let doc = WatchdogConfigDoc {
			advisors: vec![AdvisorConfig {
				name: "alpha".into(),
				..Default::default()
			}],
			..Default::default()
		};
		let mut c = AdvisorConfigOverlayComponent::new(AdvisorConfigScope::Project, doc, Cb::default(), Deps);
		let lines = c.render(80);
		assert!(!lines.is_empty());
		assert!(lines.iter().any(|l| l.contains("Advisor configuration")));
		assert!(lines.iter().any(|l| l.contains("alpha")));
		c.handle_input(&Key::Enter);
		assert_eq!(c.screen, Screen::Detail);
		c.handle_input(&Key::Esc);
		assert_eq!(c.screen, Screen::List);
	}

	#[test]
	fn advisor_config_component_add_save_remove_and_mouse() {
		let cb = Cb::default();
		let mut c = AdvisorConfigOverlayComponent::new(
			AdvisorConfigScope::User,
			WatchdogConfigDoc::default(),
			cb,
			Deps,
		);
		c.add_advisor();
		assert!(c.dirty);
		assert_eq!(c.advisors().len(), 1);
		assert_eq!(c.screen, Screen::Detail);
		c.save();
		assert!(!c.dirty);
		c.screen = Screen::List;
		c.handle_input(&Key::Down);
		c.remove_selected();
		assert!(c.advisors().is_empty());
		c.handle_mouse(&MouseEvent { kind: crate::mouse::MouseKind::ScrollDown, column: 0, row: 0, shift: false, ctrl: false, alt: false });
		assert_eq!(c.preview_scroll, 1);
	}
}
