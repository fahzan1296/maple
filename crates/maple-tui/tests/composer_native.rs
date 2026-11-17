//! Port of `l/test/composer-native.test.ts` (state parts).
//!
//! Deviations: the reference describes the native TSP composer card, working
//! row, queued messages and autocomplete nodes. The crate keeps the composer
//! as [`CustomEditor`] plus plain [`ComposerNativeState`] data for hosts that
//! render their own controls, so the port covers the startup state (thinking
//! level riding the model chip), shell/dictation state, and the editor's
//! composer-state hand-off.

use maple_tui::prompt::custom_editor::{
	ComposerNativeState, CustomEditor, DictationState, ShellKind, ShellState,
};
use maple_tui::status_line::schema::StatusLineSettings;
use maple_tui::status_line::startup::{StatusLineStartupData, StartupModel, startup_composer_state};
use maple_tui::thinking::ThinkingLevel;
use serde_json::json;

fn startup_data(model: Option<StartupModel>, level: Option<ThinkingLevel>, auto: bool) -> StatusLineStartupData {
	StatusLineStartupData {
		settings: StatusLineSettings::default(),
		git_enabled: false,
		model,
		thinking_level: level,
		auto_thinking: auto,
		fast_mode: false,
		using_subscription: false,
		auto_compact_enabled: false,
		compaction_boundaries: None,
	}
}

fn model(thinking: bool) -> StartupModel {
	StartupModel {
		value: json!({ "name": "longcat", "id": "longcat-2.5" }),
		context_window: 200_000,
		thinking,
	}
}

#[test]
fn startup_composer_rides_the_model_chip_with_the_configured_level() {
	let data = startup_data(Some(model(true)), Some(ThinkingLevel::High), false);
	let state = startup_composer_state(&data);
	assert_eq!(state.thinking.as_deref(), Some("high"));
	assert!(state.thinking_in_model, "the level draws on the model chip");
	assert!(!state.running, "no turn is running at startup");
	assert!(state.shell.is_none());
}

#[test]
fn a_model_without_thinking_has_no_composer_thinking_word() {
	let data = startup_data(Some(model(false)), Some(ThinkingLevel::High), false);
	let state = startup_composer_state(&data);
	assert_eq!(state.thinking, None);
	assert!(state.thinking_in_model);
}

#[test]
fn auto_thinking_resolves_to_the_auto_word() {
	let data = startup_data(Some(model(true)), Some(ThinkingLevel::High), true);
	assert_eq!(startup_composer_state(&data).thinking.as_deref(), Some("auto"));
}

#[test]
fn editor_roundtrips_composer_state_and_dictation_holds_a_draft() {
	let mut editor = CustomEditor::new();
	assert_eq!(editor.composer_state(), &ComposerNativeState::default());

	let state = ComposerNativeState {
		shell: Some(ShellState { kind: ShellKind::Bash, excluded: true }),
		thinking: Some("off".to_string()),
		thinking_in_model: false,
		running: true,
		dictation: Some(DictationState::Recording),
		viewing: Some(vec!["root".to_string(), "worker".to_string()]),
		title: Some("Parser work".to_string()),
	};
	editor.set_composer_state(state.clone());
	assert_eq!(editor.composer_state(), &state);

	assert!(DictationState::Recording.holds_draft());
	assert!(DictationState::Transcribing.holds_draft());
	assert!(!DictationState::Idle.holds_draft());
	assert!(!DictationState::Starting.holds_draft());
}
