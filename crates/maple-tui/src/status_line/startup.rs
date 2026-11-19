//! Status-line startup payload: validation, the startup session, and the
//! status line built from it.
//!
//! Reference: `l/src/status-line/startup.ts`.
//!
//! # Divergences
//!
//! - omptype runtime validation → hand-written [`serde_json::Value`] checks; an
//!   invalid payload yields `None`, exactly as the reference does. The schema's
//!   `"+": "delete"` (strip unknown keys) is rendered as "ignore unknown keys".
//! - `StatusLineComponent` → [`crate::status_line::component::StatusLine`]: the
//!   crate's bar is driven by `SegmentContext` instead of the host trait, so
//!   `create_startup_status_line` also fills one.
//! - `startup.segmentOptions` is validated as an object but not applied: the
//!   crate's `SegmentOptions` is a typed struct with no schema port.
//! - `thinking_level_word` reads the model's `thinking` flag off the opaque
//!   model value; the crate's copy is private to `segments.rs`.

use serde_json::{Map, Value};

use crate::prompt::custom_editor::ComposerNativeState;
use crate::status_line::component::StatusLine;
use crate::status_line::host::{
	ActiveRepoContext, CompactionBoundaries, ContextUsage, FooterHost, PullRequestOutcome,
	StatusAccountIdentity, StatusLineHost, StatusLineSession,
};
use crate::status_line::schema::{
	ContextLineMode, SegmentId, SeparatorStyle, StatusLinePreset, StatusLineSettings,
};
use crate::status_line::segments::{ModelInfo, SegmentContext, SessionInfo};
use crate::thinking::{ThinkingLevel, parse_thinking_level};

/// Facts a native status line can read before the session starts.
#[derive(Clone, Debug, PartialEq)]
pub struct StatusLineStartupData {
	/// Effective status-line settings.
	pub settings: StatusLineSettings,
	/// Whether git facts are available.
	pub git_enabled: bool,
	/// Active model.
	pub model: Option<StartupModel>,
	/// Configured thinking level.
	pub thinking_level: Option<ThinkingLevel>,
	/// Auto thinking is enabled.
	pub auto_thinking: bool,
	/// Fast mode is active.
	pub fast_mode: bool,
	/// The model bills through a subscription.
	pub using_subscription: bool,
	/// Auto-compaction is enabled.
	pub auto_compact_enabled: bool,
	/// Auto-compaction boundary positions, when known.
	pub compaction_boundaries: Option<CompactionBoundaries>,
}

/// The active model as the startup payload carries it.
#[derive(Clone, Debug, PartialEq)]
pub struct StartupModel {
	/// Catalog model object as serialized; the catalog type is not in this crate.
	pub value: Value,
	/// Context window size.
	pub context_window: u64,
	/// Whether the model supports thinking.
	pub thinking: bool,
}

impl StartupModel {
	/// Catalog model name.
	pub fn name(&self) -> &str {
		self.value.get("name").and_then(Value::as_str).unwrap_or_default()
	}

	/// Catalog model id.
	pub fn id(&self) -> &str {
		self.value.get("id").and_then(Value::as_str).unwrap_or_default()
	}
}

/// Optional string field: outer `None` means the value has the wrong type.
fn opt_str<'a>(map: &'a Map<String, Value>, key: &str) -> Option<Option<&'a str>> {
	match map.get(key) {
		None => Some(None),
		Some(Value::String(text)) => Some(Some(text.as_str())),
		Some(_) => None,
	}
}

/// Optional boolean field.
fn opt_bool(map: &Map<String, Value>, key: &str) -> Option<Option<bool>> {
	match map.get(key) {
		None => Some(None),
		Some(Value::Bool(flag)) => Some(Some(*flag)),
		Some(_) => None,
	}
}

/// Optional non-negative number field.
fn opt_u64(map: &Map<String, Value>, key: &str) -> Option<Option<u64>> {
	match map.get(key) {
		None => Some(None),
		Some(Value::Number(number)) => match number.as_f64() {
			Some(value) if value >= 0.0 => Some(Some(value as u64)),
			_ => None,
		},
		Some(_) => None,
	}
}

/// Optional number field.
fn opt_f64(map: &Map<String, Value>, key: &str) -> Option<Option<f64>> {
	match map.get(key) {
		None => Some(None),
		Some(Value::Number(number)) => number.as_f64().map(Some),
		Some(_) => None,
	}
}

/// Optional segment-id list; every entry must name a known segment.
fn opt_segment_ids(map: &Map<String, Value>, key: &str) -> Option<Option<Vec<SegmentId>>> {
	match map.get(key) {
		None => Some(None),
		Some(Value::Array(items)) => {
			let mut ids = Vec::with_capacity(items.len());
			for item in items {
				let name = item.as_str()?;
				ids.push(SegmentId::parse(name)?);
			}
			Some(Some(ids))
		}
		Some(_) => None,
	}
}

/// Validate the startup model object.
fn startup_model_from_value(value: &Value) -> Option<StartupModel> {
	let map = value.as_object()?;
	opt_str(map, "id")??;
	opt_str(map, "name")??;
	opt_str(map, "provider")??;
	opt_str(map, "api")??;
	let context_window = opt_u64(map, "contextWindow")??;
	let thinking = opt_bool(map, "thinking")?.unwrap_or(false);
	Some(StartupModel {
		value: value.clone(),
		context_window,
		thinking,
	})
}

/// Build settings from a startup payload, rejecting unknown names.
///
/// Mirrors `statusLineSettingsFromStartupData`: absent keys stay absent so the
/// preset supplies them.
fn status_line_settings_from_startup_data(value: &Value) -> Option<StatusLineSettings> {
	let map = value.as_object()?;
	let mut settings = StatusLineSettings::default();
	if let Some(preset) = opt_str(map, "preset")? {
		settings.preset = Some(StatusLinePreset::parse(preset)?);
	}
	if let Some(left) = opt_segment_ids(map, "leftSegments")? {
		settings.left_segments = Some(left);
	}
	if let Some(right) = opt_segment_ids(map, "rightSegments")? {
		settings.right_segments = Some(right);
	}
	if let Some(separator) = opt_str(map, "separator")? {
		settings.separator = Some(SeparatorStyle::parse(separator)?);
	}
	if let Some(options) = map.get("segmentOptions") {
		// Accepted as an object; the crate has no per-option schema to fill.
		options.as_object()?;
	}
	if let Some(context_line) = opt_str(map, "contextLine")? {
		settings.context_line = Some(ContextLineMode::parse(context_line)?);
	}
	if let Some(flag) = opt_bool(map, "showHookStatus")? {
		settings.show_hook_status = Some(flag);
	}
	if let Some(flag) = opt_bool(map, "sessionAccent")? {
		settings.session_accent = Some(flag);
	}
	if let Some(flag) = opt_bool(map, "transparent")? {
		settings.transparent = Some(flag);
	}
	if let Some(flag) = opt_bool(map, "compactThinkingLevel")? {
		settings.compact_thinking_level = Some(flag);
	}
	Some(settings)
}

/// Validate and read a startup status-line payload.
///
/// Returns `None` when any required field is missing or any typed field has the
/// wrong type — the reference's omptype validation rejects the whole payload.
pub fn read_status_line_startup_data(value: &Value) -> Option<StatusLineStartupData> {
	let map = value.as_object()?;
	let settings = status_line_settings_from_startup_data(map.get("settings")?)?;
	let git_enabled = opt_bool(map, "gitEnabled")??;
	let model = match map.get("model") {
		None | Some(Value::Null) => None,
		Some(value) => Some(startup_model_from_value(value)?),
	};
	let thinking_level = match opt_str(map, "thinkingLevel")? {
		Some(name) => Some(parse_thinking_level(Some(name))?),
		None => None,
	};
	let auto_thinking = opt_bool(map, "autoThinking")??;
	let fast_mode = opt_bool(map, "fastMode")??;
	let using_subscription = opt_bool(map, "usingSubscription")??;
	let auto_compact_enabled = opt_bool(map, "autoCompactEnabled")??;
	let compaction_boundaries = match map.get("compactionBoundaries") {
		None | Some(Value::Null) => None,
		Some(value) => {
			let bounds = value.as_object()?;
			let threshold_percent = opt_f64(bounds, "thresholdPercent")??;
			let speculation_percent = opt_f64(bounds, "speculationPercent")?;
			Some(CompactionBoundaries {
				threshold_percent,
				speculation_percent,
			})
		}
	};
	Some(StatusLineStartupData {
		settings,
		git_enabled,
		model,
		thinking_level,
		auto_thinking,
		fast_mode,
		using_subscription,
		auto_compact_enabled,
		compaction_boundaries,
	})
}

/// The session the startup status line reads.
#[derive(Clone, Debug, PartialEq)]
pub struct StartupSession {
	model: Option<StartupModel>,
	thinking_level: Option<ThinkingLevel>,
	auto_thinking: bool,
	using_subscription: bool,
	fast_mode: bool,
}

impl StartupSession {
	/// Wrap `data` as a [`StatusLineSession`].
	pub fn new(data: &StatusLineStartupData) -> Self {
		Self {
			model: data.model.clone(),
			thinking_level: data.thinking_level,
			auto_thinking: data.auto_thinking,
			using_subscription: data.using_subscription,
			fast_mode: data.fast_mode,
		}
	}
}

impl StatusLineSession for StartupSession {
	fn model(&self) -> Option<&Value> {
		self.model.as_ref().map(|model| &model.value)
	}

	fn thinking_level(&self) -> Option<&str> {
		self.thinking_level.map(ThinkingLevel::as_str)
	}

	fn is_auto_thinking(&self) -> bool {
		self.auto_thinking
	}

	fn is_fast_mode_active(&self) -> bool {
		self.fast_mode
	}

	fn is_using_subscription(&self, _model: &Value) -> bool {
		self.using_subscription
	}

	fn auto_resolved_thinking_level(&self) -> Option<String> {
		None
	}

	fn context_usage(&self) -> Option<ContextUsage> {
		self.model.as_ref().map(|model| ContextUsage {
			tokens: 0,
			context_window: model.context_window,
			percent: None,
		})
	}
}

/// The thinking-effort word for a startup session, or `None` when the model has
/// no thinking.
pub fn thinking_level_word(session: &dyn StatusLineSession) -> Option<String> {
	let model = session.model()?;
	let thinking = model.get("thinking").and_then(Value::as_bool).unwrap_or(false);
	if !thinking {
		return None;
	}
	if session.is_auto_thinking() {
		return Some(
			session
				.auto_resolved_thinking_level()
				.unwrap_or_else(|| "auto".to_string()),
		);
	}
	Some(
		session
			.thinking_level()
			.map(str::to_string)
			.unwrap_or_else(|| "off".to_string()),
	)
}

/// Native composer state: thinking level rides the model chip, nothing runs.
pub fn startup_composer_state(data: &StatusLineStartupData) -> ComposerNativeState {
	ComposerNativeState {
		thinking: thinking_level_word(&StartupSession::new(data)),
		thinking_in_model: true,
		running: false,
		..ComposerNativeState::default()
	}
}

/// Context for the startup status line.
///
/// Context usage is reported unknown, so the gauge shows the window without a
/// percent; `project_dir`/`home_dir` stay empty for the caller to fill.
pub fn startup_context(data: &StatusLineStartupData) -> SegmentContext {
	let model = data.model.as_ref();
	SegmentContext {
		session: SessionInfo {
			model: ModelInfo {
				name: model.map(|m| m.name().to_string()),
				id: model.map(|m| m.id().to_string()),
				thinking: model.map(|m| m.thinking).unwrap_or(false),
				thinking_level: data.thinking_level.map(|level| level.as_str().to_string()),
			},
			is_auto_thinking: data.auto_thinking,
			is_fast_mode_active: data.fast_mode,
			using_subscription: data.using_subscription,
			..SessionInfo::default()
		},
		context_window: model.map(|m| m.context_window).unwrap_or(0),
		context_percent: None,
		context_tokens: 0,
		auto_compact_enabled: data.auto_compact_enabled,
		..SegmentContext::default()
	}
}

/// Build the status line for a startup payload.
pub fn create_startup_status_line(data: &StatusLineStartupData) -> StatusLine {
	let mut status_line = StatusLine::new();
	status_line.set_settings(data.settings.clone());
	status_line.set_context(startup_context(data));
	status_line
}

/// The startup host: every runtime lookup is unavailable before the session.
#[derive(Clone, Copy, Debug)]
pub struct StartupHost<'a> {
	data: &'a StatusLineStartupData,
}

impl<'a> StartupHost<'a> {
	/// Wrap `data`.
	pub fn new(data: &'a StatusLineStartupData) -> Self {
		Self { data }
	}
}

impl StatusLineHost for StartupHost<'_> {
	fn get_settings(&self) -> StatusLineSettings {
		self.data.settings.clone()
	}

	fn git_enabled(&self) -> bool {
		self.data.git_enabled
	}

	fn codex_reset_fireworks_enabled(&self) -> bool {
		false
	}

	fn get_settings_revision(&self) -> u64 {
		0
	}

	fn get_session_settings_identity(&self, _session: &dyn StatusLineSession) -> Option<Value> {
		None
	}

	fn get_session_settings_revision(&self, _session: &dyn StatusLineSession) -> u64 {
		0
	}

	fn goal_status_in_footer(&self, _session: &dyn StatusLineSession) -> bool {
		false
	}

	fn active_account(
		&self,
		_session: &dyn StatusLineSession,
		_provider: &str,
	) -> Option<StatusAccountIdentity> {
		None
	}

	fn can_fetch_usage_reports(&self, _session: &dyn StatusLineSession) -> bool {
		false
	}

	fn fetch_usage_reports(&self, _session: &dyn StatusLineSession) -> Option<Value> {
		None
	}

	fn resolve_active_repo(&self, _cwd: &str) -> Option<ActiveRepoContext> {
		None
	}

	fn lookup_pull_request(&self, _cwd: &str) -> PullRequestOutcome {
		Default::default()
	}

	fn calculate_tokens_per_second(&self, _messages: &[Value], _is_streaming: bool) -> Option<f64> {
		None
	}

	fn limit_matches_active_account(
		&self,
		_report: &Value,
		_limit: &Value,
		_identity: &StatusAccountIdentity,
	) -> bool {
		false
	}
}

impl FooterHost for StartupHost<'_> {
	fn git_enabled(&self) -> bool {
		self.data.git_enabled
	}
}


#[cfg(test)]
mod tests {
	use super::*;

	fn payload() -> Value {
		serde_json::json!({
			"settings": {"preset": "minimal", "leftSegments": ["vim", "model"], "separator": "slash"},
			"gitEnabled": true,
			"model": {
				"id": "claude-x",
				"name": "Claude X",
				"provider": "anthropic",
				"api": "messages",
				"contextWindow": 200000,
				"thinking": true
			},
			"thinkingLevel": "high",
			"autoThinking": false,
			"fastMode": false,
			"usingSubscription": true,
			"autoCompactEnabled": true,
			"compactionBoundaries": {"thresholdPercent": 80, "speculationPercent": 70}
		})
	}

	#[test]
	fn reads_a_valid_payload() {
		let data = read_status_line_startup_data(&payload()).expect("payload");
		assert!(data.git_enabled);
		assert_eq!(data.settings.preset, Some(StatusLinePreset::Minimal));
		assert_eq!(
			data.settings.left_segments,
			Some(vec![SegmentId::Vim, SegmentId::Model])
		);
		assert_eq!(data.settings.separator, Some(SeparatorStyle::Slash));
		let model = data.model.expect("model");
		assert_eq!(model.id(), "claude-x");
		assert_eq!(model.name(), "Claude X");
		assert_eq!(model.context_window, 200_000);
		assert_eq!(data.thinking_level, Some(ThinkingLevel::High));
		assert!(data.using_subscription);
		assert!(data.auto_compact_enabled);
		let bounds = data.compaction_boundaries.expect("boundaries");
		assert_eq!(bounds.threshold_percent, 80.0);
		assert_eq!(bounds.speculation_percent, Some(70.0));
	}

	#[test]
	fn rejects_unknown_preset_and_segment_names() {
		let mut value = payload();
		value["settings"]["preset"] = Value::String("nope".to_string());
		assert_eq!(read_status_line_startup_data(&value), None);

		let mut value = payload();
		value["settings"]["rightSegments"] = serde_json::json!(["nope"]);
		assert_eq!(read_status_line_startup_data(&value), None);
	}

	#[test]
	fn rejects_wrong_types_and_missing_required_fields() {
		let mut value = payload();
		value["gitEnabled"] = Value::Bool(false);
		assert!(read_status_line_startup_data(&value).is_some());

		let mut value = payload();
		value["gitEnabled"] = Value::String("yes".to_string());
		assert_eq!(read_status_line_startup_data(&value), None);

		let mut value = payload();
		value["model"]["contextWindow"] = Value::String("big".to_string());
		assert_eq!(read_status_line_startup_data(&value), None);

		let value = Value::Null;
		assert_eq!(read_status_line_startup_data(&value), None);
	}

	#[test]
	fn model_is_optional_and_compaction_boundaries_may_be_null() {
		let mut value = payload();
		value.as_object_mut().expect("object").remove("model");
		value.as_object_mut().expect("object").remove("compactionBoundaries");
		let data = read_status_line_startup_data(&value).expect("payload");
		assert_eq!(data.model, None);
		assert_eq!(data.compaction_boundaries, None);
	}

	#[test]
	fn startup_session_reports_window_without_percent() {
		let data = read_status_line_startup_data(&payload()).expect("payload");
		let session = StartupSession::new(&data);
		let usage = session.context_usage().expect("usage");
		assert_eq!(usage.context_window, 200_000);
		assert_eq!(usage.percent, None);
		assert_eq!(session.thinking_level(), Some("high"));
		assert_eq!(thinking_level_word(&session), Some("high".to_string()));
	}

	#[test]
	fn thinking_word_is_off_without_a_level_and_none_without_thinking() {
		let mut value = payload();
		value.as_object_mut().expect("object").remove("thinkingLevel");
		let data = read_status_line_startup_data(&value).expect("payload");
		let session = StartupSession::new(&data);
		assert_eq!(thinking_level_word(&session), Some("off".to_string()));

		let mut value = payload();
		value["model"]["thinking"] = Value::Bool(false);
		let data = read_status_line_startup_data(&value).expect("payload");
		let session = StartupSession::new(&data);
		assert_eq!(thinking_level_word(&session), None);
	}

	#[test]
	fn composer_state_carries_thinking_on_the_model_chip() {
		let data = read_status_line_startup_data(&payload()).expect("payload");
		let state = startup_composer_state(&data);
		assert_eq!(state.thinking, Some("high".to_string()));
		assert!(state.thinking_in_model);
		assert!(!state.running);
	}

	#[test]
	fn startup_status_line_is_built_with_settings_and_context() {
		let data = read_status_line_startup_data(&payload()).expect("payload");
		let mut status_line = create_startup_status_line(&data);
		assert_eq!(status_line.settings().preset, Some(StatusLinePreset::Minimal));
		let context = status_line.context_mut();
		assert_eq!(context.context_window, 200_000);
		assert_eq!(context.context_percent, None);
		assert!(context.auto_compact_enabled);
		assert_eq!(context.session.model.id.as_deref(), Some("claude-x"));
		assert_eq!(context.session.model.thinking_level.as_deref(), Some("high"));
	}
}
