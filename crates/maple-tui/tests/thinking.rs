//! Port of `l/test/thinking.test.ts` (clamping/support subset).
//!
//! Divergence: TS used `@oh-my-pi/pi-catalog`/agent-core model/thinking types.
//! Rust builds `thinking::{Effort, ThinkingLevel, ThinkingModel, ModelThinking}`
//! and exercises exported clamping/support APIs.

use maple_tui::thinking::{
	assert_model_supports_effort_ceiling, clamp_thinking_level_to_ceiling, model_supports_effort_ceiling,
	Effort, ModelThinking, ThinkingLevel, ThinkingModel,
};

fn custom_reasoner(efforts: &[Effort], unverified: bool) -> ThinkingModel {
	ThinkingModel {
		provider: "custom".to_string(),
		id: "custom-reasoner".to_string(),
		reasoning: Some(true),
		thinking: Some(ModelThinking {
			default_level: None,
			unverified,
			efforts: efforts.to_vec(),
		}),
	}
}

#[test]
fn preserves_deferred_levels_under_ceiling() {
	let model = custom_reasoner(&[Effort::Low, Effort::Medium, Effort::High], false);
	for level in [ThinkingLevel::Inherit, ThinkingLevel::Off] {
		assert_eq!(
			clamp_thinking_level_to_ceiling(Some(&model), Some(level), Some(Effort::Low)),
			Some(level)
		);
	}
}

#[test]
fn caps_explicit_efforts_only_when_they_exceed_ceiling() {
	let model = custom_reasoner(&[Effort::Low, Effort::Medium, Effort::High], false);
	assert_eq!(
		clamp_thinking_level_to_ceiling(Some(&model), Some(ThinkingLevel::High), Some(Effort::Low)),
		Some(ThinkingLevel::Low)
	);
	assert_eq!(
		clamp_thinking_level_to_ceiling(Some(&model), Some(ThinkingLevel::Low), Some(Effort::Low)),
		Some(ThinkingLevel::Low)
	);
	assert_eq!(
		clamp_thinking_level_to_ceiling(Some(&model), Some(ThinkingLevel::High), Some(Effort::Max)),
		Some(ThinkingLevel::High)
	);
	assert_eq!(
		clamp_thinking_level_to_ceiling(None, Some(ThinkingLevel::High), Some(Effort::Low)),
		Some(ThinkingLevel::Low)
	);
}

#[test]
fn clamps_explicit_efforts_on_unverified_models() {
	let model = custom_reasoner(&[Effort::Low, Effort::Medium, Effort::High], true);
	assert_eq!(
		clamp_thinking_level_to_ceiling(Some(&model), Some(ThinkingLevel::High), Some(Effort::Low)),
		Some(ThinkingLevel::Low)
	);
}

#[test]
fn snaps_to_highest_supported_effort_below_ceiling() {
	let model = custom_reasoner(&[Effort::Minimal, Effort::High], false);
	assert_eq!(
		clamp_thinking_level_to_ceiling(Some(&model), Some(ThinkingLevel::High), Some(Effort::Low)),
		Some(ThinkingLevel::Minimal)
	);
}

#[test]
fn leaves_incompatible_floors_unchanged_for_caller_to_reject() {
	let model = custom_reasoner(&[Effort::Low, Effort::Medium, Effort::High], false);
	assert_eq!(
		clamp_thinking_level_to_ceiling(Some(&model), Some(ThinkingLevel::High), Some(Effort::Minimal)),
		Some(ThinkingLevel::High)
	);
	assert!(!model_supports_effort_ceiling(&model, Effort::Minimal));
	assert!(model_supports_effort_ceiling(&model, Effort::Low));
	let err = assert_model_supports_effort_ceiling(&model, Effort::Minimal).unwrap_err();
	assert!(err.contains("has no supported thinking effort at or below task.maxEffort=minimal"));
}

#[test]
fn accepts_models_with_no_controllable_effort_surface() {
	let model = ThinkingModel {
		provider: "custom".to_string(),
		id: "custom-reasoner".to_string(),
		reasoning: Some(false),
		thinking: None,
	};
	assert!(model_supports_effort_ceiling(&model, Effort::Minimal));
}
